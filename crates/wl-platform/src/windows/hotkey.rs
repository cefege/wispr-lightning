//! Global hotkey observation through a low-level keyboard hook.
//!
//! `handy-keys` owns the hook itself: `WH_KEYBOARD_LL` installed from a
//! dedicated thread that pumps messages, with the callback doing nothing but a
//! channel send. That discipline is not stylistic. Microsoft:
//!
//! > If the hook procedure times out, [...] on Windows 7 and later, the hook is
//! > silently removed without being called. **There is no way for the
//! > application to know whether the hook is removed.**
//!
//! Since no notification exists, [`WindowsHotkeys::is_healthy`] finds out by
//! experiment: it injects an inert F24 keystroke and waits to see it come back
//! round through the hook. If it does not, the listener is torn down and
//! reinstalled and the probe repeated once. This is the failure the Swift
//! version shipped with — dead hotkeys, healthy-looking app, no diagnostic.
//!
//! Everything here is plumbing; the decisions live in [`super::matching`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use handy_keys::{Key as HkKey, KeyboardListener};
use parking_lot::Mutex;
use wl_core::settings::Hotkey;

use super::matching::{edge_from, Capture, Latch};
use super::synthetic_input_in_flight;
use crate::hotkey::{HotkeyBackend, HotkeyEvent};
use crate::{PlatformError, Result};

/// How long the pump blocks waiting for a key event before servicing commands
/// and deadlines. Key delivery is not delayed by this — `recv_timeout` returns
/// as soon as an event lands.
const POLL: Duration = Duration::from_millis(20);

/// Time allowed for one liveness probe to come back round through the hook.
const PROBE_BUDGET: Duration = Duration::from_millis(250);

/// Ceiling on [`HotkeyBackend::is_healthy`]: two probe budgets plus the pump's
/// polling granularity and a little slack.
const HEALTH_BUDGET: Duration = Duration::from_millis(700);

/// How long a health verdict is reused. A probe injects a keystroke, so it is
/// not something to do on every timer tick.
const HEALTH_TTL: Duration = Duration::from_secs(5);

/// A capture left running because the settings window went away without
/// calling `end_capture` would kill the hotkey permanently, so it expires.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long `end_capture` waits for the pump to answer. Only ever one poll
/// interval in practice; the ceiling exists so a stalled pump cannot hang the
/// settings window.
const CAPTURE_REPLY_BUDGET: Duration = Duration::from_millis(500);

/// Backoff before retrying a failed hook installation.
const REINSTALL_BACKOFF: Duration = Duration::from_secs(1);

enum Command {
    Rebind(Vec<Hotkey>),
    Reset,
    BeginCapture,
    EndCapture(Sender<Option<Hotkey>>),
    Probe(Sender<bool>),
    Shutdown,
}

pub struct WindowsHotkeys {
    commands: Sender<Command>,
    events: Receiver<HotkeyEvent>,
    paused: Arc<AtomicBool>,
    health: Mutex<Health>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
struct Health {
    checked: Option<Instant>,
    healthy: bool,
}

impl WindowsHotkeys {
    /// Install the hook and start delivering transitions.
    ///
    /// Fails only if the pump thread cannot be created; a hook that refuses to
    /// install is retried in the background, because the usual cause is a
    /// transient desktop switch rather than anything permanent.
    pub fn start() -> Result<Self> {
        let (commands, command_rx) = unbounded();
        let (event_tx, events) = unbounded();
        let paused = Arc::new(AtomicBool::new(false));
        let pump_paused = Arc::clone(&paused);

        let worker = std::thread::Builder::new()
            .name("wl-hotkeys".into())
            .spawn(move || Pump::new(event_tx, pump_paused).run(&command_rx))
            .map_err(|e| PlatformError::Other(format!("could not start the hotkey pump: {e}")))?;

        Ok(Self {
            commands,
            events,
            paused,
            health: Mutex::new(Health::default()),
            worker: Mutex::new(Some(worker)),
        })
    }

    fn send(&self, command: Command) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| PlatformError::Other("the hotkey pump has stopped".into()))
    }
}

impl Drop for WindowsHotkeys {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        }
    }
}

impl HotkeyBackend for WindowsHotkeys {
    fn rebind(&self, dictate: &[Hotkey]) -> Result<()> {
        self.send(Command::Rebind(dictate.to_vec()))
    }

    fn set_paused(&self, paused: bool) {
        // Deliberately does not reset the latch: a hold that started before
        // the pause must still deliver its release, or the recorder wedges.
        self.paused.store(paused, Ordering::Relaxed);
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    fn events(&self) -> Receiver<HotkeyEvent> {
        self.events.clone()
    }

    fn reset(&self) {
        let _ = self.send(Command::Reset);
    }

    fn begin_capture(&self) {
        let _ = self.send(Command::BeginCapture);
    }

    fn end_capture(&self) -> Option<Hotkey> {
        let (tx, rx) = bounded(1);
        self.send(Command::EndCapture(tx)).ok()?;
        rx.recv_timeout(CAPTURE_REPLY_BUDGET).ok().flatten()
    }

    /// Whether the hook is still delivering events.
    ///
    /// Blocks for up to [`HEALTH_BUDGET`] on a cache miss and injects one
    /// inert keystroke, so call it from a timer off the UI thread rather than
    /// per frame. Verdicts are reused for [`HEALTH_TTL`].
    fn is_healthy(&self) -> bool {
        let mut health = self.health.lock();
        if health.checked.is_some_and(|at| at.elapsed() < HEALTH_TTL) {
            return health.healthy;
        }
        let (tx, rx) = bounded(1);
        let healthy = match self.send(Command::Probe(tx)) {
            Ok(()) => rx.recv_timeout(HEALTH_BUDGET).unwrap_or(false),
            Err(_) => false,
        };
        *health = Health {
            checked: Some(Instant::now()),
            healthy,
        };
        if !healthy {
            tracing::error!("the global keyboard hook is not delivering events");
        }
        healthy
    }
}

// ---------------------------------------------------------------------------
// Pump
// ---------------------------------------------------------------------------

struct CaptureSession {
    capture: Capture,
    deadline: Instant,
}

struct ProbeSession {
    reply: Sender<bool>,
    deadline: Instant,
    /// Whether the hook has already been reinstalled for this probe.
    reinstalled: bool,
}

struct Pump {
    listener: Option<KeyboardListener>,
    retry_at: Instant,
    dictate: Vec<Hotkey>,
    latch: Latch,
    capture: Option<CaptureSession>,
    probe: Option<ProbeSession>,
    paused: Arc<AtomicBool>,
    events: Sender<HotkeyEvent>,
}

impl Pump {
    fn new(events: Sender<HotkeyEvent>, paused: Arc<AtomicBool>) -> Self {
        Self {
            listener: None,
            retry_at: Instant::now(),
            dictate: Vec::new(),
            latch: Latch::default(),
            capture: None,
            probe: None,
            paused,
            events,
        }
    }

    fn run(mut self, commands: &Receiver<Command>) {
        loop {
            while let Ok(command) = commands.try_recv() {
                if matches!(command, Command::Shutdown) {
                    return;
                }
                self.handle_command(command);
            }

            self.ensure_listener();
            match self.listener.as_ref().map(|l| l.recv_timeout(POLL)) {
                Some(Ok(event)) => self.handle_key(&event),
                Some(Err(handy_keys::Error::Timeout)) => {}
                Some(Err(e)) => {
                    // The listener's own thread is gone; a fresh one is the
                    // only recovery. Backing off keeps a listener that dies
                    // immediately on every install from spinning this thread.
                    tracing::warn!(error = %e, "keyboard listener stopped; reinstalling");
                    self.listener = None;
                    self.retry_at = Instant::now() + REINSTALL_BACKOFF;
                }
                None => std::thread::sleep(POLL),
            }

            self.expire_capture();
            self.expire_probe();
        }
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Rebind(dictate) => {
                self.dictate = dictate;
                // A held key can no longer be attributed to a binding that may
                // not exist any more.
                self.latch.reset();
            }
            Command::Reset => self.latch.reset(),
            Command::BeginCapture => {
                self.capture = Some(CaptureSession {
                    capture: Capture::default(),
                    deadline: Instant::now() + CAPTURE_TIMEOUT,
                });
            }
            Command::EndCapture(reply) => {
                let recorded = self.capture.take().and_then(|s| s.capture.finish());
                let _ = reply.send(recorded);
            }
            Command::Probe(reply) => self.start_probe(reply),
            Command::Shutdown => {}
        }
    }

    fn handle_key(&mut self, event: &handy_keys::KeyEvent) {
        // The probe check comes first: its keystroke is synthetic, so the
        // guard below would otherwise swallow the very evidence we want.
        if self.probe.is_some() && event.key == Some(PROBE_KEY) {
            if let Some(probe) = self.probe.take() {
                let _ = probe.reply.send(true);
            }
            return;
        }
        // Our own paste and typing travel through the same hook. Without this
        // a Ctrl+V would retrigger a Control push-to-talk binding.
        if synthetic_input_in_flight() {
            return;
        }

        // The chord guard, before matching and before the `edge_from` filter:
        // the key that gives `Ctrl+C` away is `C`, which the settings model
        // cannot express and which therefore has no `Edge` of its own.
        //
        // Its position *after* the synthetic check is load-bearing: paste and
        // Natural Mode keystrokes return through this hook as ordinary key
        // downs and must not abort the dictation they belong to.
        for hotkey_event in self.latch.guard(event) {
            if self.events.send(hotkey_event).is_err() {
                return;
            }
        }

        let Some(edge) = edge_from(event) else {
            // A key the settings model cannot express. It matches no binding,
            // but a capture in progress has to hear about it: recording
            // "Ctrl+Shift" when the user pressed Ctrl+Shift+K would bind
            // something they never chose.
            if event.key.is_some() && event.is_key_down {
                if let Some(session) = self.capture.as_mut() {
                    session.capture.reject();
                }
            }
            return;
        };

        let suppress = self.paused.load(Ordering::Relaxed) || self.capture.is_some();
        for hotkey_event in self.latch.apply(&edge, &self.dictate, suppress) {
            if self.events.send(hotkey_event).is_err() {
                return;
            }
        }
        if let Some(session) = self.capture.as_mut() {
            session.capture.observe(&edge);
        }
    }

    fn ensure_listener(&mut self) {
        if self.listener.is_some() || Instant::now() < self.retry_at {
            return;
        }
        // Non-blocking mode: the Swift app's `NSEvent` monitors are passive
        // observers, so the key must still reach the focused application.
        match KeyboardListener::new() {
            Ok(listener) => {
                tracing::info!("global keyboard hook installed");
                self.listener = Some(listener);
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not install the keyboard hook; will retry");
                self.retry_at = Instant::now() + REINSTALL_BACKOFF;
            }
        }
    }

    fn reinstall(&mut self) {
        self.listener = None;
        self.retry_at = Instant::now();
        self.ensure_listener();
    }

    fn start_probe(&mut self, reply: Sender<bool>) {
        if self.listener.is_none() {
            self.reinstall();
        }
        if let Err(e) = super::injector::send_probe_keystroke() {
            // Injection itself failed, which means the desktop is not ours to
            // drive; the hook is dark for the same reason.
            tracing::warn!(error = %e, "liveness probe could not be injected");
            let _ = reply.send(false);
            return;
        }
        self.probe = Some(ProbeSession {
            reply,
            deadline: Instant::now() + PROBE_BUDGET,
            reinstalled: false,
        });
    }

    fn expire_probe(&mut self) {
        let Some(probe) = self.probe.take() else {
            return;
        };
        if Instant::now() < probe.deadline {
            self.probe = Some(probe);
            return;
        }
        if probe.reinstalled {
            let _ = probe.reply.send(false);
            return;
        }
        tracing::warn!("keyboard hook did not observe the liveness probe; reinstalling");
        self.reinstall();
        if super::injector::send_probe_keystroke().is_err() {
            let _ = probe.reply.send(false);
            return;
        }
        self.probe = Some(ProbeSession {
            reply: probe.reply,
            deadline: Instant::now() + PROBE_BUDGET,
            reinstalled: true,
        });
    }

    fn expire_capture(&mut self) {
        if self
            .capture
            .as_ref()
            .is_some_and(|session| Instant::now() >= session.deadline)
        {
            tracing::debug!("hotkey capture abandoned; resuming normal matching");
            self.capture = None;
        }
    }
}

/// The key the liveness probe injects.
///
/// F24 is the conventional inert key for exactly this purpose — it exists on
/// no shipping keyboard, and AutoHotkey-style tools use the same range as a
/// mask. While a probe is in flight its event is consumed, so a user who has
/// deliberately bound F24 loses at most that one press during a health check.
const PROBE_KEY: HkKey = HkKey::F24;
