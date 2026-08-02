//! The orchestrator: hotkeys in, injected text out.
//!
//! Everything else in this app is a capability. This is the module that knows
//! the *order* those capabilities run in, and that order is the product. A
//! faithful port of `Sources/WisprLightning/App/AppDelegate.swift`, which
//! interleaved the recording lifecycle with AppKit callbacks; here it is a
//! single-consumer actor so the sequencing is stated once and can be tested.
//!
//! Three structural rules make that possible:
//!
//! 1. **The state machine is not re-implemented.** [`wl_core::fsm::Machine`] is
//!    pure and already covers the tap-vs-hold timing table. This module only
//!    executes the [`Action`]s it emits.
//! 2. **Every dependency is a trait.** No Tauri type appears below, so the
//!    whole pipeline runs headless against fakes — see `pipeline/tests.rs`.
//! 3. **One task owns the FSM.** Recording state is mutated only by the actor
//!    loop, so there is no lock ordering to get wrong. Work that can be slow
//!    (network, accessibility, OCR, injection, disk) is spawned off it.
//!
//! The one piece of shared mutable state outside the actor is [`Pending`]: the
//! audio awaiting a transcript. It has to be reachable from the transcription
//! task *and* from the overlay's Retry/Save/Dismiss buttons, so it lives
//! behind a mutex rather than in the actor.
//!
//! Layout: this file holds wiring and shared helpers, [`actor`] owns recording
//! state, and [`transcribe`] owns everything after the key is released.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use wl_core::consts::MIN_PACKETS;
use wl_core::db::{DictionaryStore, HistoryStore, NewTranscript};
use wl_core::fsm::{
    elapsed_label, Action, Event, Machine, PressBehavior, State, Tick, WarningState,
};
use wl_core::settings::Settings;
use wl_platform::audio::{AudioCapture, CaptureFault, StartOutcome};
use wl_platform::hotkey::{Binding, HotkeyBackend, Transition};
use wl_platform::sound::{Cue, SoundPlayer};
use wl_platform::{AppInfo, AppKind, InjectMode, Platform};
use wl_providers::{
    AppContext, DictationContext, DictionaryContext, ProviderError, TranscriptResult,
    TranscriptionProvider,
};

use crate::spool::{Recovered, Spool};
use crate::ui::{Elapsed, OverlayState, Ui};

mod actor;
mod transcribe;

#[cfg(test)]
mod tests;

use actor::Actor;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Automatic retries after the first attempt, for retryable failures only.
/// Three total attempts, which is what the overlay's "2 of 3" reports.
const MAX_AUTO_RETRIES: u32 = 2;

/// A recording that produced no packets at all but ran longer than this was
/// almost certainly talking to a microphone that had gone away.
const DEAD_MIC_THRESHOLD: Duration = Duration::from_secs(1);

/// Screen-context lines sent upstream. Fixed by the protocol (WSS-009).
const OCR_MAX_LINES: usize = 50;

/// Overlay copy authored here rather than by a provider. OVL-039 fixes the
/// first five verbatim; [`MSG_SILENT_INPUT`] is new, for a failure mode the
/// Swift app could not detect (AUD-037).
const MSG_MIC_UNAVAILABLE: &str = "Mic unavailable";
const MSG_MIC_NOT_RESPONDING: &str = "Mic not responding";
const MSG_TIMED_OUT: &str = "Timed out";
const MSG_RECOVERED: &str = "Recovered unsent recording";
const MSG_SILENT_INPUT: &str = "No sound from the mic \u{2014} check microphone privacy settings";

/// Every duration the pipeline waits on, in one place so tests can compress
/// them. Real time is not a thing a unit test should have to spend.
#[derive(Debug, Clone, Copy)]
pub struct Timings {
    /// Gap between automatic transcription retries.
    pub retry_delay: Duration,
    /// Coalescing window before re-opening the microphone after a device event.
    pub rearm_debounce: Duration,
    /// How often capture faults are drained.
    pub fault_poll: Duration,
    /// Elapsed-time tick while recording.
    pub tick: Duration,
    /// Floor of the end-to-end processing deadline.
    pub processing_timeout_base: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            retry_delay: Duration::from_millis(1500),
            rearm_debounce: Duration::from_millis(150),
            fault_poll: Duration::from_millis(250),
            tick: Duration::from_secs(1),
            processing_timeout_base: Duration::from_secs(30),
        }
    }
}

/// Deadline for the whole transcription attempt sequence, retries included.
///
/// Scales with the recording because a ten-minute dictation legitimately takes
/// longer to come back than a five-second one.
fn processing_timeout(base: Duration, duration_secs: f64) -> Duration {
    base + Duration::from_secs_f64((duration_secs * 0.5).max(0.0))
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

/// Everything the pipeline drives. Assembled by the app shell from its own
/// state; kept as a plain struct so this module never has to know that
/// `AppState` (or Tauri) exists.
pub struct PipelineDeps {
    pub settings: Arc<RwLock<Settings>>,
    pub platform: Platform,
    pub audio: Arc<dyn AudioCapture>,
    pub sound: Arc<dyn SoundPlayer>,
    pub hotkeys: Arc<dyn HotkeyBackend>,
    pub provider: Arc<RwLock<Arc<dyn TranscriptionProvider>>>,
    pub history: Arc<HistoryStore>,
    pub dictionary: Arc<DictionaryStore>,
    pub spool: Arc<Spool>,
    pub ui: Arc<dyn Ui>,
    /// Where "Save" writes a recovered recording.
    pub downloads_dir: PathBuf,
    pub timings: Timings,
}

/// What the overlay's buttons ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Retry,
    /// Export the preserved audio as a WAV the user can keep.
    Save,
    Discard,
    /// The ✕ on the recording pill: abandon this dictation outright.
    ///
    /// Distinct from every other path that ends a recording, because it is the
    /// one case where discarding the audio is CORRECT. "Never lose audio"
    /// exists for failures — a crash, a dead socket, exhausted retries — where
    /// the user still wants the words they said. Cancel is the user telling us
    /// they do not. Keeping the spooled artifact would greet them on the next
    /// launch with "Recovered unsent recording", offering to transcribe and
    /// paste the thing they just cancelled. Intent, not failure.
    Cancel,
}

impl OverlayAction {
    /// Parse the IPC spelling.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "retry" => Some(Self::Retry),
            "save" => Some(Self::Save),
            "dismiss" => Some(Self::Discard),
            "cancel" => Some(Self::Cancel),
            _ => None,
        }
    }
}

/// Handle onto the running orchestrator. Cloneable state lives in `Deps`; this
/// is just the mailbox.
pub struct Pipeline {
    tx: mpsc::UnboundedSender<Command>,
    capturing: Arc<AtomicBool>,
}

impl Pipeline {
    /// Start the actor plus its hotkey and device-fault feeds.
    ///
    /// Must be called from inside a Tokio runtime. The returned handle keeps
    /// the actor alive; dropping every clone shuts it down.
    pub fn spawn(deps: PipelineDeps) -> Arc<Self> {
        let capturing = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::unbounded_channel();

        let hotkey_events = deps.hotkeys.events();
        let fault_poll = deps.timings.fault_poll;

        let audio = deps.audio.clone();
        let deps = Arc::new(Deps {
            settings: deps.settings,
            platform: deps.platform,
            audio: deps.audio,
            sound: deps.sound,
            hotkeys: deps.hotkeys,
            provider: deps.provider,
            history: deps.history,
            dictionary: deps.dictionary,
            spool: deps.spool,
            ui: deps.ui,
            downloads_dir: deps.downloads_dir,
            timings: deps.timings,
            pending: Mutex::new(None),
            transcribing: AtomicBool::new(false),
        });

        // The hotkey backend hands out a blocking crossbeam receiver, so it
        // gets an OS thread rather than a Tokio task. It ends when the backend
        // drops its sender.
        let hotkey_tx = tx.clone();
        std::thread::Builder::new()
            .name("wl-hotkeys".into())
            .spawn(move || {
                while let Ok(event) = hotkey_events.recv() {
                    if hotkey_tx.send(Command::Hotkey(event)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn hotkey forwarder");

        let fault_tx = tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(fault_poll).await;
                let faults = audio.take_faults();
                if !faults.is_empty() && fault_tx.send(Command::Faults(faults)).is_err() {
                    break;
                }
            }
        });

        // Seed and prime the dictionary off the launch path: the first
        // dictation must not pay for three cold queries while the user is
        // already talking.
        let warming = deps.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(error) = warming.dictionary.seed_defaults(None) {
                tracing::warn!(%error, "could not seed the default dictionary");
            }
            if let Err(error) = warming.dictionary.warm_up() {
                tracing::warn!(%error, "could not warm the dictionary caches");
            }
        });

        Actor::start(deps, tx.clone(), rx, capturing.clone());

        Arc::new(Self { tx, capturing })
    }

    /// Offer a recording that outlived the process that made it.
    pub fn offer_recovery(&self, recovered: Recovered) {
        self.send(Command::Recovery(Box::new(recovered)));
    }

    pub fn overlay_action(&self, action: OverlayAction) {
        self.send(Command::Overlay(action));
    }

    /// Re-read settings: rebind hotkeys, re-point the microphone, reload the
    /// sound pack.
    pub fn settings_changed(&self) {
        self.send(Command::SettingsChanged);
    }

    /// Suppress hotkey handling while the settings window is recording a new
    /// binding. The backend suppresses events too; this is the second lock on
    /// the door, because a stray press here would start a recording the user
    /// never asked for.
    pub fn set_capturing_hotkey(&self, capturing: bool) {
        self.capturing.store(capturing, Ordering::SeqCst);
    }

    /// Abandon any in-flight recording. Used for system sleep and shutdown.
    pub fn abort(&self) {
        self.send(Command::Abort);
    }

    fn send(&self, command: Command) {
        if self.tx.send(command).is_err() {
            tracing::warn!("pipeline actor is gone; command dropped");
        }
    }
}

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

enum Command {
    Hotkey(wl_platform::hotkey::HotkeyEvent),
    /// A scheduled stop matured. Carries the generation it was armed with, so
    /// a timer outlived by its session is ignored.
    StopTimer(u64),
    Tick,
    Faults(Vec<CaptureFault>),
    Rearm,
    Overlay(OverlayAction),
    SettingsChanged,
    /// Boxed: this variant is far larger than the others and would otherwise
    /// set the size of every message in the queue.
    Recovery(Box<Recovered>),
    Abort,
}

/// Shared, immutable-after-construction dependency set.
struct Deps {
    settings: Arc<RwLock<Settings>>,
    platform: Platform,
    audio: Arc<dyn AudioCapture>,
    sound: Arc<dyn SoundPlayer>,
    hotkeys: Arc<dyn HotkeyBackend>,
    provider: Arc<RwLock<Arc<dyn TranscriptionProvider>>>,
    history: Arc<HistoryStore>,
    dictionary: Arc<DictionaryStore>,
    spool: Arc<Spool>,
    ui: Arc<dyn Ui>,
    downloads_dir: PathBuf,
    timings: Timings,
    pending: Mutex<Option<Pending>>,
    /// Guards against two transcription drivers running for one recording,
    /// e.g. a Retry press landing while an automatic retry is still in flight.
    transcribing: AtomicBool,
}

/// Audio that has been recorded but not yet turned into text.
///
/// Held until a transcript comes back. Never dropped on failure — the user
/// already said the words and cannot say them again identically.
struct Pending {
    packets: Arc<Vec<Vec<i16>>>,
    app: AppInfo,
    ocr: Vec<String>,
    ax: Vec<String>,
    /// Stable across retries so the backend can deduplicate and so a retried
    /// dictation replaces its history row instead of duplicating it.
    transcript_id: String,
    /// Filled in asynchronously once the spool write lands.
    spool_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Resume whatever we paused. Unconditional: the media layer already knows
/// whether it interrupted anything, and gating this on the current value of
/// `mute_music` would strand the user's music if they toggled it mid-take.
fn resume_music(deps: &Arc<Deps>) {
    let media = deps.platform.media.clone();
    tokio::task::spawn_blocking(move || media.resume());
}

/// Drop the pending recording *and* its spooled file. Only ever called when
/// the audio is genuinely finished with: transcribed, aborted, or dismissed.
fn discard_pending(deps: &Arc<Deps>) {
    let Some(pending) = deps.pending.lock().take() else {
        return;
    };
    if let Some(path) = pending.spool_path {
        deps.spool.delete(&path);
    }
}

/// Natural Mode types character by character; everything else pastes. This is
/// a user setting, not a fallback chain — the injector never picks for itself.
fn inject_mode(settings: &Settings) -> InjectMode {
    if settings.natural_mode_enabled {
        InjectMode::Natural {
            chars_per_second: settings.natural_mode_speed.chars_per_second(),
        }
    } else {
        InjectMode::Paste
    }
}

/// Insert `text` at the caret.
///
/// The overlay is forced to [`OverlayState::Inserting`] first, and that is why
/// every injection goes through this one function rather than calling the
/// injector directly. The reference implementation learned this the hard way:
/// without a reset, whatever the pill was showing bleeds through — a Retrying
/// yellow band, or the Retry/Save/✕ buttons of a failed attempt — and sits on
/// screen while text is being typed. Its `CLAUDE.md` records the precondition
/// explicitly, once per inject call site. Making the reset structural instead
/// removes the chance of adding a fourth call site that forgets.
async fn inject(deps: &Arc<Deps>, text: &str, mode: InjectMode) {
    deps.ui.set_overlay(OverlayState::Inserting);

    let injector = deps.platform.injector.clone();
    let owned = text.to_string();
    match tokio::task::spawn_blocking(move || injector.inject(&owned, mode)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!(%error, "text injection failed"),
        Err(error) => tracing::error!(%error, "injection task failed"),
    }
}
