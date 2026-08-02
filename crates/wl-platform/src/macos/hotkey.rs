//! Global hotkey observation on macOS.
//!
//! Built on `handy-keys` 0.3.3, which owns the CGEventTap: it creates the tap
//! on its own thread with its own CFRunLoop, tracks side-specific modifier
//! state from keycodes rather than the shared flag bits, and — critically —
//! re-enables the tap from **inside** the callback when macOS delivers
//! `TapDisabledByTimeout` / `TapDisabledByUserInput`, reconciling its tracked
//! modifiers against `CGEventSource::flags_state` because events were missed
//! while the tap was dead. Polling `CGEventTapIsEnabled` on a timer is the
//! wrong recovery: those recurring WindowServer RPCs leak kernel IPC vouchers
//! (see `docs/parity/research-input.md` §2).
//!
//! What is *not* delegated is binding matching. This module drives the raw
//! `KeyboardListener` rather than `handy_keys::HotkeyManager` for two reasons:
//! the manager exposes only `recv()`/`try_recv()`, so owning it would mean
//! polling forever on a menu-bar app that runs all day, and the press/release
//! semantics this app needs (pause asymmetry, latch, capture mode) are not
//! expressible on top of it.
//!
//! Nor is the synthetic-input guard (HTK-011). The Swift listener gated every
//! trigger on `isLocalHIDEvent`; see `accepts_key_event` below for what survives
//! the move to a `handy-keys` tap and what does not.
//!
//! One rule here has no Swift ancestor at all: the chord guard, which cancels
//! a held modifier-only binding the moment the user types under it. See
//! [`crate::chord`] for why that is a fix rather than a regression.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use objc2_core_graphics::{CGEventFlags, CGEventSource, CGEventSourceStateID};
use tracing::{debug, info, warn};
use wl_core::settings::hotkey::{Hotkey, Modifiers, TriggerKey};

use super::synthetic_input_in_flight;
use crate::chord::interrupts_chord;
use crate::hotkey::{Binding, HotkeyBackend, HotkeyEvent, Transition};
use crate::{PlatformError, Result};

/// How long the worker parks waiting for input before it services commands and
/// runs the liveness check. Key events wake it immediately, so this bounds
/// command latency only.
const TICK: Duration = Duration::from_millis(50);

/// How often to re-check that the tap could still be delivering events.
///
/// `AXIsProcessTrusted` is a TCC query, not a WindowServer RPC, so this is
/// safe to run on a timer — unlike `CGEventTapIsEnabled`.
const HEALTH_INTERVAL: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

/// Translate the app's portable hotkey into the one `handy-keys` matches on.
///
/// Fails for a hotkey that cannot fire at all, and for an F-key beyond F20 —
/// macOS virtual keycodes stop there, so F21..F24 are unreachable from Mac
/// hardware even though the enum can express them.
pub fn to_handy(hotkey: &Hotkey) -> Result<handy_keys::Hotkey> {
    let key = hotkey.key.map(to_handy_key).transpose()?;
    handy_keys::Hotkey::new(to_handy_modifiers(hotkey.modifiers), key)
        .map_err(|e| PlatformError::Other(format!("unusable hotkey {}: {e}", hotkey.label())))
}

/// The inverse, used by the settings hotkey recorder to persist what the user
/// pressed.
pub fn from_handy(hotkey: &handy_keys::Hotkey) -> Option<Hotkey> {
    let key = match hotkey.key {
        Some(k) => Some(from_handy_key(k)?),
        None => None,
    };
    let result = Hotkey {
        modifiers: from_handy_modifiers(hotkey.modifiers),
        key,
    };
    result.is_valid().then_some(result)
}

/// Side-specific modifier flags, one bit at a time.
///
/// `handy-keys` also has compound "either side" aliases; they are never
/// produced because the app treats each side as an independent trigger.
fn to_handy_modifiers(modifiers: Modifiers) -> handy_keys::Modifiers {
    let mut out = handy_keys::Modifiers::empty();
    for (ours, theirs) in MODIFIER_PAIRS {
        if modifiers.contains(ours) {
            out |= theirs;
        }
    }
    out
}

fn from_handy_modifiers(modifiers: handy_keys::Modifiers) -> Modifiers {
    let mut out = Modifiers::NONE;
    for (ours, theirs) in MODIFIER_PAIRS {
        if modifiers.contains(theirs) {
            out |= ours;
        }
    }
    out
}

const MODIFIER_PAIRS: [(Modifiers, handy_keys::Modifiers); 9] = [
    (Modifiers::CTRL_LEFT, handy_keys::Modifiers::CTRL_LEFT),
    (Modifiers::CTRL_RIGHT, handy_keys::Modifiers::CTRL_RIGHT),
    // "Option" on the Mac keyboard is the Alt position.
    (Modifiers::ALT_LEFT, handy_keys::Modifiers::OPT_LEFT),
    (Modifiers::ALT_RIGHT, handy_keys::Modifiers::OPT_RIGHT),
    // "Command" is the Meta position.
    (Modifiers::META_LEFT, handy_keys::Modifiers::CMD_LEFT),
    (Modifiers::META_RIGHT, handy_keys::Modifiers::CMD_RIGHT),
    (Modifiers::SHIFT_LEFT, handy_keys::Modifiers::SHIFT_LEFT),
    (Modifiers::SHIFT_RIGHT, handy_keys::Modifiers::SHIFT_RIGHT),
    (Modifiers::FN, handy_keys::Modifiers::FN),
];

fn to_handy_key(key: TriggerKey) -> Result<handy_keys::Key> {
    use handy_keys::Key;
    let mapped = match key {
        TriggerKey::Return => Key::Return,
        TriggerKey::Space => Key::Space,
        TriggerKey::Escape => Key::Escape,
        TriggerKey::Tab => Key::Tab,
        TriggerKey::F(n) => match n {
            1 => Key::F1,
            2 => Key::F2,
            3 => Key::F3,
            4 => Key::F4,
            5 => Key::F5,
            6 => Key::F6,
            7 => Key::F7,
            8 => Key::F8,
            9 => Key::F9,
            10 => Key::F10,
            11 => Key::F11,
            12 => Key::F12,
            13 => Key::F13,
            14 => Key::F14,
            15 => Key::F15,
            16 => Key::F16,
            17 => Key::F17,
            18 => Key::F18,
            19 => Key::F19,
            20 => Key::F20,
            _ => {
                return Err(PlatformError::Unsupported(
                    "function keys above F20 have no macOS virtual keycode",
                ))
            }
        },
    };
    Ok(mapped)
}

fn from_handy_key(key: handy_keys::Key) -> Option<TriggerKey> {
    use handy_keys::Key;
    let mapped = match key {
        Key::Return | Key::KeypadEnter => TriggerKey::Return,
        Key::Space => TriggerKey::Space,
        Key::Escape => TriggerKey::Escape,
        Key::Tab => TriggerKey::Tab,
        Key::F1 => TriggerKey::F(1),
        Key::F2 => TriggerKey::F(2),
        Key::F3 => TriggerKey::F(3),
        Key::F4 => TriggerKey::F(4),
        Key::F5 => TriggerKey::F(5),
        Key::F6 => TriggerKey::F(6),
        Key::F7 => TriggerKey::F(7),
        Key::F8 => TriggerKey::F(8),
        Key::F9 => TriggerKey::F(9),
        Key::F10 => TriggerKey::F(10),
        Key::F11 => TriggerKey::F(11),
        Key::F12 => TriggerKey::F(12),
        Key::F13 => TriggerKey::F(13),
        Key::F14 => TriggerKey::F(14),
        Key::F15 => TriggerKey::F(15),
        Key::F16 => TriggerKey::F(16),
        Key::F17 => TriggerKey::F(17),
        Key::F18 => TriggerKey::F(18),
        Key::F19 => TriggerKey::F(19),
        Key::F20 => TriggerKey::F(20),
        // Letters, digits, punctuation and mouse buttons are outside the
        // app's trigger vocabulary.
        _ => return None,
    };
    Some(mapped)
}

/// The CoreGraphics flag bit a modifier contributes.
///
/// Flags are side-agnostic: Left and Right Control share `MaskControl`. That
/// is a macOS property, not a simplification — the Swift original had the same
/// blind spot — and it only matters for the stuck-key reconcile below.
fn cg_flag(modifier: Modifiers) -> CGEventFlags {
    match modifier {
        Modifiers::CTRL_LEFT | Modifiers::CTRL_RIGHT => CGEventFlags::MaskControl,
        Modifiers::ALT_LEFT | Modifiers::ALT_RIGHT => CGEventFlags::MaskAlternate,
        Modifiers::META_LEFT | Modifiers::META_RIGHT => CGEventFlags::MaskCommand,
        Modifiers::SHIFT_LEFT | Modifiers::SHIFT_RIGHT => CGEventFlags::MaskShift,
        Modifiers::FN => CGEventFlags::MaskSecondaryFn,
        _ => CGEventFlags::empty(),
    }
}

/// Whether a key event the tap reported is allowed to drive a binding.
///
/// Two independent rejections, one per hazard:
///
/// * `source_pid` is the event's `kCGEventSourceUnixProcessID`. Zero means the
///   kernel posted it from real HID hardware; anything else means some process
///   called `CGEventPost` — which is how Universal Control re-posts the other
///   Mac's modifier state, and how any app on the machine could start a
///   dictation. `None` means there was no backing `CGEvent`, which is accepted,
///   matching `HotkeyListener.isLocalHIDEvent`'s
///   `guard let cg = event.cgEvent else { return true }`.
/// * `self_injecting` is [`synthetic_input_in_flight`]: paste and Natural Mode
///   typing are posted at `kCGHIDEventTap` and come back through this tap,
///   indistinguishable from the user's keyboard.
///
/// # `source_pid` is always `None` today
///
/// `handy-keys` owns the `CGEventTap`, decodes the `CGEvent` inside its own
/// callback and hands us a `KeyEvent { modifiers, key, is_key_down,
/// changed_modifier }` — the raw event and therefore the PID field never leave
/// that crate (0.3.3 is the latest release; there is no accessor). So the first
/// rejection is specified and tested but cannot fire, and **HTK-011's
/// foreign-process half is not closed**: another app posting a Left Control
/// still starts a dictation. Recovering it needs either an upstream field on
/// `KeyEvent` or owning the tap ourselves, and a second tap is not an option —
/// two taps see the same event independently, so ours could not suppress
/// anything in theirs without a timing correlation worse than the window below.
///
/// The self-injection half *is* closed, by the same armed window the Windows
/// backend uses, for the same reason (`dwExtraInfo` is equally invisible to
/// `handy-keys`). That is the half that bites in practice, because we are the
/// process that synthesizes input into our own tap all day.
fn accepts_key_event(source_pid: Option<i64>, self_injecting: bool) -> bool {
    !self_injecting && matches!(source_pid, None | Some(0))
}

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

enum Command {
    Rebind {
        dictate: Vec<handy_keys::Hotkey>,
        labels: Vec<String>,
    },
    Reset,
    BeginCapture,
    EndCapture(Sender<Option<Hotkey>>),
    Shutdown,
}

pub struct MacHotkeys {
    commands: Sender<Command>,
    events: Receiver<HotkeyEvent>,
    paused: Arc<AtomicBool>,
    healthy: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MacHotkeys {
    /// Start the listener thread.
    ///
    /// Deliberately succeeds even when Accessibility has not been granted: the
    /// tap cannot be created yet, [`HotkeyBackend::is_healthy`] reports
    /// `false`, and the worker retries so the app starts working the moment
    /// the user flips the switch — without a relaunch.
    pub fn new() -> Result<Self> {
        let (commands_tx, commands_rx) = crossbeam_channel::unbounded();
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        let paused = Arc::new(AtomicBool::new(false));

        // Created here rather than on the worker so `is_healthy` is truthful
        // the instant this returns; a caller that renders the tray icon from
        // it would otherwise flash "hotkeys are dead" on every launch.
        let listener = match handy_keys::KeyboardListener::new() {
            Ok(listener) => {
                info!("global hotkey listener active");
                Some(listener)
            }
            Err(err) => {
                warn!(%err, "hotkeys are off until Accessibility is granted");
                None
            }
        };
        let healthy = Arc::new(AtomicBool::new(listener.is_some()));

        let worker = {
            let paused = Arc::clone(&paused);
            let healthy = Arc::clone(&healthy);
            thread::Builder::new()
                .name("wl-hotkeys".into())
                .spawn(move || Worker::new(events_tx, paused, healthy, listener).run(&commands_rx))
                .map_err(PlatformError::Io)?
        };

        Ok(Self {
            commands: commands_tx,
            events: events_rx,
            paused,
            healthy,
            worker: Some(worker),
        })
    }

    fn send(&self, command: Command) {
        // The worker only goes away during shutdown, when nothing is waiting
        // on the effect of a command anyway.
        let _ = self.commands.send(command);
    }
}

impl Drop for MacHotkeys {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl HotkeyBackend for MacHotkeys {
    fn rebind(&self, dictate: &[Hotkey]) -> Result<()> {
        let mut labels = Vec::new();
        let mut mapped = Vec::new();
        for hotkey in dictate {
            let handy = to_handy(hotkey)?;
            if !mapped.contains(&handy) {
                labels.push(hotkey.label());
                mapped.push(handy);
            }
        }
        self.send(Command::Rebind {
            dictate: mapped,
            labels,
        });
        Ok(())
    }

    fn set_paused(&self, paused: bool) {
        if self.paused.swap(paused, Ordering::SeqCst) != paused {
            info!(paused, "hotkey pause toggled");
            // Clear the latch so a physically-held key is not stuck across the
            // toggle.
            self.send(Command::Reset);
        }
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    fn events(&self) -> Receiver<HotkeyEvent> {
        self.events.clone()
    }

    fn reset(&self) {
        self.send(Command::Reset);
    }

    fn begin_capture(&self) {
        self.send(Command::BeginCapture);
    }

    fn end_capture(&self) -> Option<Hotkey> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send(Command::EndCapture(tx));
        // A missing answer must read as "cancelled", never as a hung UI, so
        // the wait is bounded a few worker ticks out.
        rx.recv_timeout(TICK * 4).ok().flatten()
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

/// One active binding.
struct Registered {
    binding: Binding,
    hotkey: handy_keys::Hotkey,
}

struct Worker {
    events: Sender<HotkeyEvent>,
    paused: Arc<AtomicBool>,
    healthy: Arc<AtomicBool>,
    listener: Option<handy_keys::KeyboardListener>,
    registered: Vec<Registered>,
    /// Indices into `registered` whose press we delivered and whose release we
    /// therefore still owe the app.
    latched: HashSet<usize>,
    /// Bindings the chord guard cancelled, keyed by value rather than by
    /// index so a rebind mid-hold cannot re-arm one by renumbering.
    ///
    /// A binding in here is physically held but must not start anything: the
    /// user is mid-shortcut. Entries leave only when the modifiers actually
    /// come up, either on the event that lifts them or through
    /// [`Worker::reconcile_latched`] if the tap missed it.
    disarmed: HashSet<handy_keys::Hotkey>,
    /// `Some(slot)` while the settings UI is recording a new binding.
    capture: Option<Option<Hotkey>>,
    last_health_check: Instant,
}

impl Worker {
    fn new(
        events: Sender<HotkeyEvent>,
        paused: Arc<AtomicBool>,
        healthy: Arc<AtomicBool>,
        listener: Option<handy_keys::KeyboardListener>,
    ) -> Self {
        // A listener we already have needs no immediate re-check; one we
        // failed to create should be retried on the very first pass.
        let last_health_check = if listener.is_some() {
            Instant::now()
        } else {
            Instant::now()
                .checked_sub(HEALTH_INTERVAL)
                .unwrap_or_else(Instant::now)
        };
        Self {
            events,
            paused,
            healthy,
            listener,
            registered: Vec::new(),
            latched: HashSet::new(),
            disarmed: HashSet::new(),
            capture: None,
            last_health_check,
        }
    }

    fn run(mut self, commands: &Receiver<Command>) {
        loop {
            self.check_health();

            // Park on the tap when we have one, otherwise on a plain sleep;
            // either way the thread is asleep between events.
            let event = match &self.listener {
                Some(listener) => match listener.recv_timeout(TICK) {
                    Ok(event) => Some(event),
                    Err(handy_keys::Error::Timeout) => None,
                    Err(err) => {
                        warn!(%err, "the keyboard listener stopped; will rebuild");
                        self.listener = None;
                        self.healthy.store(false, Ordering::SeqCst);
                        None
                    }
                },
                None => {
                    thread::sleep(TICK);
                    None
                }
            };

            if let Some(event) = event {
                self.on_key_event(&event);
            }

            while let Ok(command) = commands.try_recv() {
                if matches!(command, Command::Shutdown) {
                    return;
                }
                self.on_command(command);
            }
        }
    }

    fn on_command(&mut self, command: Command) {
        match command {
            Command::Rebind { dictate, labels } => {
                self.registered = dictate
                    .into_iter()
                    .map(|hotkey| Registered {
                        binding: Binding::Dictate,
                        hotkey,
                    })
                    .collect();
                // Held keys can no longer be attributed to bindings that may
                // not exist any more. The chord guard's disarm survives on
                // purpose: it is keyed by hotkey value, so a binding that came
                // through the rebind unchanged is still physically held and
                // still must not fire.
                self.latched.clear();
                info!(triggers = %labels.join(" or "), "hotkey bindings replaced");
            }
            // Only the latch. The chord guard's disarm deliberately outlives a
            // reset — the pipeline resets from its abort path, which is the
            // path a chord abort itself takes, so forgetting here would let
            // the still-held Control of a `Ctrl+C` start a fresh dictation the
            // moment the user let go. `reconcile_latched` is what eventually
            // clears a disarm the tap never saw released.
            Command::Reset => self.latched.clear(),
            Command::BeginCapture => {
                self.capture = Some(None);
                // Anything held right now would otherwise be reported as
                // released the moment the user lets go mid-capture.
                self.latched.clear();
            }
            Command::EndCapture(reply) => {
                let _ = reply.send(self.capture.take().flatten());
            }
            Command::Shutdown => unreachable!("handled by the caller"),
        }
    }

    /// Match one raw key event against the active bindings.
    ///
    /// Mirrors the Swift listener's state machine: a press latches, and only a
    /// latched binding can emit a release. That is what makes the pause
    /// asymmetry work — a press swallowed while paused never latches, so its
    /// release is swallowed too, while a press that happened *before* the
    /// pause still gets its release and can stop the recording.
    ///
    /// The one rule with no Swift ancestor is the chord guard, which cancels a
    /// held modifier-only binding when the user types under it. See
    /// [`crate::chord`], and [`Worker::abort_chords`] for the mechanics.
    ///
    /// The synthetic-input guard runs ahead of everything, capture included: a
    /// key of our own arriving while the settings recorder is open would
    /// otherwise be bound as the user's new hotkey. Releases are swallowed
    /// too, as on Windows — consistent, because a press that was swallowed
    /// never latched, and [`Worker::reconcile_latched`] is the backstop for a
    /// real release that lands while the synthetic-input window is open.
    fn on_key_event(&mut self, event: &handy_keys::KeyEvent) {
        self.dispatch(event, synthetic_input_in_flight());
    }

    /// [`Worker::on_key_event`] with the synthetic-input verdict handed in.
    ///
    /// Split out purely so tests can exercise both sides of it without arming
    /// the process-wide window, which is a global and would race every other
    /// test in the binary.
    fn dispatch(&mut self, event: &handy_keys::KeyEvent, self_injecting: bool) {
        // `handy-keys` decoded the CGEvent inside its own tap callback, so the
        // PID is not available here; `None` is the case Swift also accepts.
        if !accepts_key_event(None, self_injecting) {
            return;
        }
        if let Some(slot) = &mut self.capture {
            if event.is_key_down && slot.is_none() {
                *slot = from_handy(&handy_keys::Hotkey {
                    modifiers: event.modifiers,
                    key: event.key,
                });
            }
            // Capture consumes the event: matching bindings here would start a
            // recording while the user is trying to rebind.
            return;
        }

        // Let the chord guard re-arm anything whose modifiers have actually
        // come up. Ahead of matching, so the event that frees a binding can
        // also be the one that legitimately starts it again.
        self.rearm_chords(event.modifiers);

        if event.is_key_down {
            // The chord guard runs before the pause check, for the reason
            // releases ignore it too: a hold that began before the pause must
            // still be able to end.
            if event.key.is_some_and(interrupts_chord) {
                self.abort_chords();
            }
            if self.paused.load(Ordering::SeqCst) {
                return;
            }
            for index in 0..self.registered.len() {
                let entry = &self.registered[index];
                if entry.hotkey.modifiers.matches(event.modifiers)
                    && entry.hotkey.key == event.key
                    && !self.disarmed.contains(&entry.hotkey)
                    && self.latched.insert(index)
                {
                    self.emit(entry.binding, Transition::Pressed);
                }
            }
            return;
        }

        // A key-up releases the binding that owns that key. A modifier change
        // releases every latched binding whose modifiers no longer match — but
        // not one whose modifiers still hold, so tapping Shift while a
        // Control-only binding is held does not stop the recording.
        let releasing: Vec<usize> = self
            .latched
            .iter()
            .copied()
            .filter(|&index| {
                let hotkey = &self.registered[index].hotkey;
                match event.key {
                    Some(_) => hotkey.key == event.key,
                    None => !hotkey.modifiers.matches(event.modifiers),
                }
            })
            .collect();
        for index in releasing {
            self.latched.remove(&index);
            self.emit(self.registered[index].binding, Transition::Released);
        }
    }

    /// Cancel every held modifier-only binding: the user just typed, so the
    /// modifiers under their fingers are a shortcut prefix, not a trigger.
    ///
    /// The binding is dropped from the latch *and* remembered in `disarmed`,
    /// so the modifier release that follows neither reports a stop the app
    /// never started nor starts a fresh dictation. Bindings that carry a
    /// trigger key are left alone — they cannot be typed by accident.
    ///
    /// Idempotent, which matters because key auto-repeat delivers the same
    /// key-down over and over: the second call finds nothing latched.
    fn abort_chords(&mut self) {
        let interrupted: Vec<usize> = self
            .latched
            .iter()
            .copied()
            .filter(|&index| is_modifier_only(&self.registered[index].hotkey))
            .collect();
        for index in interrupted {
            let hotkey = self.registered[index].hotkey;
            self.latched.remove(&index);
            self.disarmed.insert(hotkey);
            debug!("a keystroke arrived under a held modifier binding; abandoning the hold");
            self.emit(self.registered[index].binding, Transition::Aborted);
        }
    }

    /// Let a cancelled binding fire again, once its modifiers are demonstrably
    /// no longer held.
    ///
    /// This is the "release and press again" half of the guard. Without it the
    /// Control of a `Ctrl+C` would start a dictation the instant any other
    /// modifier moved while it was still down.
    fn rearm_chords(&mut self, modifiers: handy_keys::Modifiers) {
        if self.disarmed.is_empty() {
            return;
        }
        self.disarmed
            .retain(|hotkey| hotkey.modifiers.matches(modifiers));
    }

    fn emit(&self, binding: Binding, transition: Transition) {
        debug!(?binding, ?transition, "hotkey");
        let _ = self.events.send(HotkeyEvent {
            binding,
            transition,
        });
    }

    /// Keep the tap alive, and un-stick anything the tap missed.
    fn check_health(&mut self) {
        if self.last_health_check.elapsed() < HEALTH_INTERVAL {
            return;
        }
        self.last_health_check = Instant::now();

        // Losing Accessibility kills the tap with no pseudo-event, so the
        // in-callback recovery inside `handy-keys` cannot see it. Tearing the
        // listener down and rebuilding once trust returns is the documented
        // remedy for a tap that has gone inert.
        if !handy_keys::check_accessibility() {
            if self.listener.take().is_some() {
                warn!("accessibility permission was revoked; hotkeys are off");
            }
            self.healthy.store(false, Ordering::SeqCst);
            return;
        }

        if self.listener.is_none() {
            match handy_keys::KeyboardListener::new() {
                Ok(listener) => {
                    info!("global hotkey listener active");
                    self.listener = Some(listener);
                    self.latched.clear();
                }
                Err(err) => {
                    debug!(%err, "could not start the keyboard listener");
                    self.healthy.store(false, Ordering::SeqCst);
                    return;
                }
            }
        }

        self.healthy.store(true, Ordering::SeqCst);
        self.reconcile_latched();
    }

    /// Release anything latched whose keys are no longer physically held, and
    /// let the chord guard go of anything it is still holding down on.
    ///
    /// A tap that was disabled and re-enabled loses every event in between. If
    /// the missing one was the release of the push-to-talk key, the app would
    /// sit in "recording" forever. Checking against the OS's own view of the
    /// modifier state is exactly the reconcile `handy-keys` performs for its
    /// internal tracker; this is the same fix one level up.
    ///
    /// A disarmed binding needs the same backstop for the mirror-image reason:
    /// if the tap missed the key-up that would have re-armed it, the hotkey
    /// would stay dead until the user happened to move some other modifier.
    ///
    /// Only runs while something is latched or disarmed, so it costs nothing
    /// at idle.
    fn reconcile_latched(&mut self) {
        if self.latched.is_empty() && self.disarmed.is_empty() {
            return;
        }
        let flags = CGEventSource::flags_state(CGEventSourceStateID::CombinedSessionState);
        self.disarmed.retain(|hotkey| modifiers_held(hotkey, flags));
        let stuck: Vec<usize> = self
            .latched
            .iter()
            .copied()
            .filter(|&index| !modifiers_held(&self.registered[index].hotkey, flags))
            .collect();
        for index in stuck {
            warn!("releasing a hotkey the event tap never reported as released");
            self.latched.remove(&index);
            self.emit(self.registered[index].binding, Transition::Released);
        }
    }
}

/// Whether every modifier this binding needs is physically down right now.
///
/// A binding with no modifiers at all (a bare F-key) has nothing to check
/// against the flags, so it is left latched for the real key-up to clear.
fn modifiers_held(hotkey: &handy_keys::Hotkey, flags: CGEventFlags) -> bool {
    if hotkey.modifiers.is_empty() {
        return true;
    }
    let mut required = CGEventFlags::empty();
    for (ours, theirs) in MODIFIER_PAIRS {
        if hotkey.modifiers.contains(theirs) {
            required |= cg_flag(ours);
        }
    }
    flags.contains(required)
}

/// Whether this binding is nothing but modifiers, which is what makes it
/// indistinguishable from the prefix of a keyboard shortcut and therefore the
/// only kind the chord guard applies to.
///
/// The number of modifiers is deliberately **not** part of the test, and this
/// predicate is not too broad. `Ctrl+Shift+S` and `Ctrl+Shift+Esc` are every
/// bit as real as `Ctrl+C`, so a user who rebinds to a two-modifier
/// combination has exactly the same hazard; a modifier-count check here would
/// protect only the shipped default and leave everyone else exposed. Nor does
/// it cost a `Ctrl+Shift` binding anything: it still starts, holds and stops
/// normally, and the guard speaks up only once a *third*, non-modifier key
/// goes down — at which point the user was typing a shortcut, not dictating.
///
/// Bindings that carry a trigger key (`Ctrl+Space`, a bare `F13`) are the
/// exclusion: they cannot be typed by accident, so the guard leaves them be.
fn is_modifier_only(hotkey: &handy_keys::Hotkey) -> bool {
    hotkey.key.is_none() && !hotkey.modifiers.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::{begin_synthetic_input, SYNTHETIC_GUARD};

    #[test]
    fn a_bare_left_control_survives_the_round_trip() {
        let ours = Hotkey::modifier(Modifiers::CTRL_LEFT);
        let theirs = to_handy(&ours).expect("left control is representable");
        assert_eq!(theirs.modifiers, handy_keys::Modifiers::CTRL_LEFT);
        assert_eq!(theirs.key, None, "a bare modifier must carry no key");
        assert_eq!(from_handy(&theirs), Some(ours));
    }

    /// Independent left- and right-side bindings must remain distinguishable.
    #[test]
    fn left_and_right_modifiers_stay_distinct() {
        let left = to_handy(&Hotkey::modifier(Modifiers::CTRL_LEFT)).expect("mappable");
        let right = to_handy(&Hotkey::modifier(Modifiers::CTRL_RIGHT)).expect("mappable");
        assert_ne!(left, right);
        assert_ne!(left.modifiers, handy_keys::Modifiers::CTRL);
    }

    #[test]
    fn every_modifier_round_trips() {
        for (ours, _) in MODIFIER_PAIRS {
            let hotkey = Hotkey::modifier(ours);
            let theirs = to_handy(&hotkey).expect("mappable");
            assert_eq!(from_handy(&theirs), Some(hotkey), "{}", hotkey.label());
        }
    }

    #[test]
    fn every_trigger_key_round_trips() {
        let keys = [
            TriggerKey::Return,
            TriggerKey::Space,
            TriggerKey::Escape,
            TriggerKey::Tab,
        ]
        .into_iter()
        .chain((1..=20).map(TriggerKey::F));

        for key in keys {
            let hotkey = Hotkey::combo(Modifiers::SHIFT_LEFT, key);
            let theirs = to_handy(&hotkey).expect("mappable");
            assert_eq!(from_handy(&theirs), Some(hotkey), "{}", hotkey.label());
        }
    }

    #[test]
    fn a_multi_modifier_chord_maps_every_side() {
        let ours = Hotkey::combo(
            Modifiers::CTRL_LEFT | Modifiers::SHIFT_RIGHT | Modifiers::FN,
            TriggerKey::Space,
        );
        let theirs = to_handy(&ours).expect("mappable");
        assert_eq!(
            theirs.modifiers,
            handy_keys::Modifiers::CTRL_LEFT
                | handy_keys::Modifiers::SHIFT_RIGHT
                | handy_keys::Modifiers::FN
        );
        assert_eq!(from_handy(&theirs), Some(ours));
    }

    #[test]
    fn a_hotkey_that_can_never_fire_is_rejected() {
        let empty = Hotkey {
            modifiers: Modifiers::NONE,
            key: None,
        };
        assert!(to_handy(&empty).is_err());
    }

    /// macOS virtual keycodes stop at F20; pretending otherwise would register
    /// a binding that silently never fires.
    #[test]
    fn function_keys_above_f20_are_rejected() {
        let hotkey = Hotkey::combo(Modifiers::NONE, TriggerKey::F(21));
        assert!(matches!(
            to_handy(&hotkey),
            Err(PlatformError::Unsupported(_))
        ));
    }

    /// Keys outside the app's vocabulary must not be silently coerced into one
    /// that is — the recorder has to reject them so the UI can say so.
    #[test]
    fn an_unsupported_recorded_key_yields_nothing() {
        let letter = handy_keys::Hotkey {
            modifiers: handy_keys::Modifiers::CMD_LEFT,
            key: Some(handy_keys::Key::K),
        };
        assert_eq!(from_handy(&letter), None);
    }

    #[test]
    fn a_modifier_binding_counts_as_held_only_while_its_flag_is_set() {
        let ctrl = to_handy(&Hotkey::modifier(Modifiers::CTRL_LEFT)).expect("mappable");
        assert!(modifiers_held(&ctrl, CGEventFlags::MaskControl));
        assert!(modifiers_held(
            &ctrl,
            CGEventFlags::MaskControl | CGEventFlags::MaskShift
        ));
        assert!(!modifiers_held(&ctrl, CGEventFlags::empty()));
        assert!(!modifiers_held(&ctrl, CGEventFlags::MaskShift));
    }

    /// The stuck-key reconcile must not fire for a binding whose modifiers it
    /// cannot observe, or a bare F-key would be released the instant it is
    /// pressed.
    #[test]
    fn a_binding_without_modifiers_is_never_reconciled_away() {
        let f5 = to_handy(&Hotkey::combo(Modifiers::NONE, TriggerKey::F(5))).expect("mappable");
        assert!(modifiers_held(&f5, CGEventFlags::empty()));
    }

    /// PID zero is the kernel's HID stack — the only source the Swift listener
    /// treated as the user physically pressing a key.
    #[test]
    fn a_kernel_posted_event_drives_a_binding() {
        assert!(accepts_key_event(Some(0), false));
    }

    /// Anything else came out of some process's `CGEventPost`: another app, or
    /// Universal Control replaying the other Mac's modifier state.
    #[test]
    fn an_event_posted_by_another_process_is_rejected() {
        assert!(!accepts_key_event(Some(1), false));
        assert!(!accepts_key_event(Some(945), false));
        // The field is signed and the Swift test is `== 0`, not `> 0`.
        assert!(!accepts_key_event(Some(-1), false));
    }

    /// `isLocalHIDEvent` defaults to true when `event.cgEvent` is nil, and so
    /// must this: refusing an event we cannot classify would make every hotkey
    /// depend on a field that is not always there.
    #[test]
    fn an_event_with_no_source_pid_is_accepted() {
        assert!(accepts_key_event(None, false));
    }

    /// The half that actually fires: our own paste and Natural Mode keystrokes
    /// come back through this tap with our PID, not zero.
    #[test]
    fn our_own_injection_is_rejected_while_the_window_is_armed() {
        assert!(!accepts_key_event(None, true));
        assert!(!accepts_key_event(Some(0), true));
    }

    /// The window has to close on its own, or one paste would kill hotkeys for
    /// the rest of the session.
    ///
    /// Arms the real process-wide guard, so it is the only test in this binary
    /// allowed to call [`begin_synthetic_input`].
    #[test]
    fn the_armed_window_expires() {
        begin_synthetic_input(SYNTHETIC_GUARD);
        assert!(
            synthetic_input_in_flight(),
            "the window must be open the instant it is armed"
        );
        assert!(!accepts_key_event(Some(0), synthetic_input_in_flight()));

        thread::sleep(SYNTHETIC_GUARD * 2);
        assert!(!synthetic_input_in_flight());
        assert!(accepts_key_event(Some(0), synthetic_input_in_flight()));
    }

    // -- The chord guard -------------------------------------------------
    //
    // Driven straight through `Worker::dispatch`, which needs no tap and no
    // Accessibility grant: the worker only touches the OS from `run` and
    // `check_health`, neither of which is called here. `dispatch` also takes
    // the synthetic-input verdict as an argument, so none of this races the
    // process-wide window that `the_armed_window_expires` owns.

    /// One worker with a channel to read back what it decided.
    struct Keyboard {
        worker: Worker,
        events: Receiver<HotkeyEvent>,
    }

    impl Keyboard {
        fn new(dictate: &[Hotkey]) -> Self {
            let (tx, rx) = crossbeam_channel::unbounded();
            let mut worker = Worker::new(
                tx,
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                None,
            );
            worker.on_command(Command::Rebind {
                dictate: dictate
                    .iter()
                    .map(|hotkey| to_handy(hotkey).expect("mappable"))
                    .collect(),
                labels: Vec::new(),
            });
            Self { worker, events: rx }
        }

        /// A modifier going down or up. `now` is the state *after* the change,
        /// which is what the tap reports.
        fn modifiers(&mut self, now: handy_keys::Modifiers, down: bool) -> Vec<Transition> {
            self.feed(now, None, down, false)
        }

        /// An ordinary key going down or up under `now`.
        fn key(
            &mut self,
            now: handy_keys::Modifiers,
            key: handy_keys::Key,
            down: bool,
        ) -> Vec<Transition> {
            self.feed(now, Some(key), down, false)
        }

        /// A key synthesized by paste or Natural Mode coming through the tap.
        fn synthetic_key(
            &mut self,
            now: handy_keys::Modifiers,
            key: handy_keys::Key,
        ) -> Vec<Transition> {
            self.feed(now, Some(key), true, true)
        }

        fn feed(
            &mut self,
            now: handy_keys::Modifiers,
            key: Option<handy_keys::Key>,
            down: bool,
            self_injecting: bool,
        ) -> Vec<Transition> {
            self.worker.dispatch(
                &handy_keys::KeyEvent {
                    modifiers: now,
                    key,
                    is_key_down: down,
                    // Never read by the worker; the tap's own tracker is the
                    // only consumer of this field.
                    changed_modifier: None,
                },
                self_injecting,
            );
            self.events
                .try_iter()
                .map(|event| {
                    assert_eq!(event.binding, Binding::Dictate);
                    event.transition
                })
                .collect()
        }
    }

    const CTRL: handy_keys::Modifiers = handy_keys::Modifiers::CTRL_LEFT;
    const CTRL_SHIFT: handy_keys::Modifiers =
        handy_keys::Modifiers::CTRL_LEFT.union(handy_keys::Modifiers::SHIFT_LEFT);
    const NONE: handy_keys::Modifiers = handy_keys::Modifiers::empty();

    fn ctrl() -> Keyboard {
        Keyboard::new(&[Hotkey::modifier(Modifiers::CTRL_LEFT)])
    }

    /// The bug, in one test. Without the guard the `C` is invisible here, the
    /// Control release stops the recording, and half a second of room noise is
    /// transcribed into whatever the user was copying from.
    #[test]
    fn a_keystroke_under_a_held_bare_modifier_abandons_the_hold() {
        let mut kb = ctrl();
        assert_eq!(kb.modifiers(CTRL, true), [Transition::Pressed]);
        assert_eq!(
            kb.key(CTRL, handy_keys::Key::C, true),
            [Transition::Aborted]
        );
    }

    /// The rest of `Ctrl+C`: neither the key-up nor the modifier-up may report
    /// anything, or the pipeline would either stop a recording it already
    /// abandoned or start a new one on the way out.
    #[test]
    fn an_abandoned_hold_stays_silent_until_the_user_presses_again() {
        let mut kb = ctrl();
        kb.modifiers(CTRL, true);
        kb.key(CTRL, handy_keys::Key::C, true);

        assert_eq!(kb.key(CTRL, handy_keys::Key::C, false), []);
        assert_eq!(kb.modifiers(NONE, false), []);

        // ...and the hotkey is not dead, just disarmed for that one hold.
        assert_eq!(kb.modifiers(CTRL, true), [Transition::Pressed]);
    }

    /// The disarm has to be a real piece of state, not a side effect of the
    /// latch being empty: with Control still down, any other modifier moving
    /// re-satisfies a bare-Control binding and would start a dictation the
    /// user never asked for.
    #[test]
    fn another_modifier_moving_cannot_revive_an_abandoned_hold() {
        let mut kb = ctrl();
        kb.modifiers(CTRL, true);
        assert_eq!(
            kb.key(CTRL, handy_keys::Key::C, true),
            [Transition::Aborted]
        );

        assert_eq!(kb.modifiers(CTRL_SHIFT, true), []);
        assert_eq!(kb.modifiers(CTRL, false), []);
        assert_eq!(kb.key(CTRL, handy_keys::Key::V, true), []);
    }

    /// Key auto-repeat delivers the same key-down over and over. Only the
    /// first may be reported, or the pipeline sees a stream of aborts.
    #[test]
    fn auto_repeat_reports_the_abort_once() {
        let mut kb = ctrl();
        kb.modifiers(CTRL, true);
        assert_eq!(
            kb.key(CTRL, handy_keys::Key::C, true),
            [Transition::Aborted]
        );
        assert_eq!(kb.key(CTRL, handy_keys::Key::C, true), []);
        assert_eq!(kb.key(CTRL, handy_keys::Key::C, true), []);
    }

    /// The pipeline's abort path calls `HotkeyBackend::reset`, and a chord
    /// abort *is* an abort — so the cleanup runs on every guard firing. If
    /// reset cleared the disarm it would hand the still-held Control straight
    /// back, and the guard would close the hole and reopen it in one breath.
    #[test]
    fn the_pipelines_reset_does_not_re_arm_a_held_modifier() {
        let mut kb = ctrl();
        kb.modifiers(CTRL, true);
        assert_eq!(
            kb.key(CTRL, handy_keys::Key::C, true),
            [Transition::Aborted]
        );

        kb.worker.on_command(Command::Reset);

        assert_eq!(kb.modifiers(CTRL_SHIFT, true), [], "still held, still off");
        assert_eq!(kb.modifiers(CTRL, false), []);
        assert_eq!(kb.key(CTRL, handy_keys::Key::C, false), []);
        assert_eq!(kb.modifiers(NONE, false), []);

        // Only a genuinely new press brings it back.
        assert_eq!(kb.modifiers(CTRL, true), [Transition::Pressed]);
    }

    /// Settings changes rebind on every save, so a rebind mid-hold must not be
    /// a way back either. Keying the disarm by hotkey value rather than by
    /// index is what makes this hold.
    #[test]
    fn a_rebind_does_not_re_arm_a_held_modifier() {
        let mut kb = ctrl();
        kb.modifiers(CTRL, true);
        assert_eq!(
            kb.key(CTRL, handy_keys::Key::C, true),
            [Transition::Aborted]
        );

        kb.worker.on_command(Command::Rebind {
            dictate: vec![to_handy(&Hotkey::modifier(Modifiers::CTRL_LEFT)).expect("mappable")],
            labels: Vec::new(),
        });

        assert_eq!(kb.modifiers(CTRL_SHIFT, true), []);
        assert_eq!(kb.modifiers(CTRL, false), []);
    }

    /// A binding with a trigger key cannot be typed by accident, so the guard
    /// has no business touching it — including when the user types under it.
    #[test]
    fn a_binding_with_a_trigger_key_is_left_alone() {
        let mut kb = Keyboard::new(&[Hotkey::combo(Modifiers::CTRL_LEFT, TriggerKey::Space)]);
        assert_eq!(kb.modifiers(CTRL, true), []);
        assert_eq!(
            kb.key(CTRL, handy_keys::Key::Space, true),
            [Transition::Pressed]
        );

        assert_eq!(kb.key(CTRL, handy_keys::Key::C, true), [], "not guarded");
        assert_eq!(
            kb.key(CTRL, handy_keys::Key::Space, false),
            [Transition::Released]
        );
    }

    /// A two-modifier binding is guarded — `Ctrl+Shift+S` is as real as
    /// `Ctrl+C` — but the guard costs it nothing until a third key lands.
    #[test]
    fn a_two_modifier_binding_still_starts_and_stops_normally() {
        let mut kb = Keyboard::new(&[Hotkey::modifier(
            Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT,
        )]);
        assert_eq!(kb.modifiers(CTRL, true), [], "half a chord is not a chord");
        assert_eq!(kb.modifiers(CTRL_SHIFT, true), [Transition::Pressed]);
        assert_eq!(kb.modifiers(CTRL, false), [Transition::Released]);
    }

    #[test]
    fn a_two_modifier_binding_is_guarded_too() {
        let mut kb = Keyboard::new(&[Hotkey::modifier(
            Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT,
        )]);
        kb.modifiers(CTRL, true);
        kb.modifiers(CTRL_SHIFT, true);
        assert_eq!(
            kb.key(CTRL_SHIFT, handy_keys::Key::S, true),
            [Transition::Aborted]
        );
    }

    /// Hands-free, mechanically: the modifier is not held, so there is no hold
    /// to abandon and a `Ctrl+C` is somebody else's shortcut. The pipeline
    /// gates on the state machine as well, for the instant during a locking
    /// press when the key genuinely is still down.
    #[test]
    fn a_shortcut_with_nothing_held_is_none_of_the_guards_business() {
        let mut kb = ctrl();
        kb.modifiers(CTRL, true);
        assert_eq!(kb.modifiers(NONE, false), [Transition::Released]);

        assert_eq!(kb.key(NONE, handy_keys::Key::C, true), []);
        assert_eq!(kb.key(NONE, handy_keys::Key::C, false), []);
    }

    /// Control-clicking mid-sentence is deliberate. macOS routes exactly these
    /// through the same channel whenever a modifier is held, which is when the
    /// guard is armed, so this is not a hypothetical.
    #[test]
    fn a_modifier_click_does_not_abandon_the_hold() {
        let mut kb = ctrl();
        kb.modifiers(CTRL, true);
        assert_eq!(kb.key(CTRL, handy_keys::Key::MouseLeft, true), []);
        assert_eq!(kb.modifiers(NONE, false), [Transition::Released]);
    }

    /// Our own paste and Natural Mode keystrokes arrive here as ordinary key
    /// downs, and none may throw away an in-progress dictation.
    #[test]
    fn our_own_keystrokes_never_abandon_a_hold() {
        let mut kb = ctrl();
        assert_eq!(kb.modifiers(CTRL, true), [Transition::Pressed]);

        assert_eq!(kb.synthetic_key(CTRL, handy_keys::Key::C), []);
        assert_eq!(kb.synthetic_key(CTRL, handy_keys::Key::V), []);
        assert_eq!(kb.synthetic_key(CTRL, handy_keys::Key::Space), []);

        // Untouched: the hold ends as a hold, and the take is transcribed.
        assert_eq!(kb.modifiers(NONE, false), [Transition::Released]);
    }
}
