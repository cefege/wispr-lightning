//! Pipeline behaviour, end to end, against fakes.
//!
//! The orchestrator is the one module where the *order* of operations is the
//! product, so these tests assert observable sequences rather than internal
//! state. Every dependency writes into one shared [`Log`], which makes
//! "start cue, then record, then stop cue, then inject, then hide" an
//! assertion about a vector instead of a story in a comment.
//!
//! Timings are compressed through [`Timings`] rather than faked with a mock
//! clock: the pipeline's waits are real `tokio::time` sleeps, and shrinking
//! them keeps the concurrency exactly as it ships.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use crossbeam_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};

use wl_core::consts::CHUNK_SAMPLES;
use wl_core::db::{Database, DictionaryStore, HistoryStore};
use wl_core::settings::{EmailSignature, Hotkey, Settings};
use wl_platform::audio::{AudioCapture, CaptureFault, InputDevice, StartOutcome};
use wl_platform::hotkey::{Binding, HotkeyBackend, HotkeyEvent, Transition};
use wl_platform::sound::{Cue, SoundPlayer};
use wl_platform::{
    AppInfo, AppKind, ClipboardSnapshot, ForegroundApp, InjectMode, MediaControl, Permission,
    PermissionState, Permissions, Platform, PlatformError, ScreenText, TextInjector,
};
use wl_providers::{
    DictationContext, DictationSession, ProviderError, TranscriptResult, TranscriptionProvider,
};

use super::*;
use crate::ui::{Elapsed, OverlayState, Ui};

// ---------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------

/// One thing the pipeline did, in the order it did it.
#[derive(Debug, Clone, PartialEq)]
enum Ev {
    /// The frontmost app was sampled, carrying the name it reported. Ordering
    /// matters more than the value: several rows turn on *when* this happens.
    ForegroundRead(String),
    Cue(Cue),
    Overlay(OverlayState),
    /// An elapsed readout reached the overlay, in the event stream rather than
    /// only in [`Log::elapsed`] so "the tick starts last and stops first" is a
    /// statement about the same sequence as everything else.
    Tick(Elapsed),
    Indicator(bool),
    LastTranscription(String),
    Notify(String),
    CaptureStart,
    CaptureStop(usize),
    CaptureRelease,
    CapturePrewarm,
    Inject(String),
    ClipboardSnapshot,
    ClipboardRestore,
    ProviderStart,
    Transcribe,
    ProviderReset,
    MusicPause,
    MusicResume,
    ReadFocused,
    Ocr,
    /// Escape reached the injector and stopped Natural Mode typing.
    CancelTyping,
    /// "Undo last dictation" fired.
    Undo,
}

#[derive(Default)]
struct Log {
    events: Mutex<Vec<Ev>>,
    /// Elapsed readouts, kept apart so a long recording does not drown the
    /// sequence assertions in ticks.
    elapsed: Mutex<Vec<Elapsed>>,
    /// Audio levels, kept apart for the same reason and more so: these arrive
    /// at ~25 Hz. Recorded rather than dropped because "the pipeline stopped
    /// pumping levels" is a regression a silent fake would hide.
    levels: Mutex<Vec<f32>>,
}

impl Log {
    fn push(&self, event: Ev) {
        self.events.lock().push(event);
    }

    fn snapshot(&self) -> Vec<Ev> {
        self.events.lock().clone()
    }

    fn count(&self, event: &Ev) -> usize {
        self.events.lock().iter().filter(|e| *e == event).count()
    }

    fn contains(&self, event: &Ev) -> bool {
        self.events.lock().iter().any(|e| e == event)
    }

    /// Where `event` first landed in the sequence. `None` when it never did.
    ///
    /// For work the pipeline spawns rather than performs inline: the exact
    /// interleaving is up to the scheduler, but "after the microphone opened"
    /// is still a hard guarantee and is what these rows are about.
    fn index_of(&self, event: &Ev) -> Option<usize> {
        self.events.lock().iter().position(|e| e == event)
    }

    /// The recorded events, filtered to those the caller cares about. Lets a
    /// test state an exact sequence without listing background chatter.
    fn sequence(&self, keep: impl Fn(&Ev) -> bool) -> Vec<Ev> {
        self.events
            .lock()
            .iter()
            .filter(|e| keep(e))
            .cloned()
            .collect()
    }

    fn injected(&self) -> Vec<String> {
        self.events
            .lock()
            .iter()
            .filter_map(|e| match e {
                Ev::Inject(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn overlays(&self) -> Vec<OverlayState> {
        self.events
            .lock()
            .iter()
            .filter_map(|e| match e {
                Ev::Overlay(state) => Some(state.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Poll until `predicate` holds. Failing this is always a real failure: the
/// pipeline is entirely event-driven, so anything it is going to do it does
/// within a few scheduler turns of being asked.
async fn wait_for(what: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Give queued work a chance to run without waiting for a specific outcome.
async fn settle() {
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

struct RecordingUi {
    log: Arc<Log>,
}

impl Ui for RecordingUi {
    fn set_overlay(&self, state: OverlayState) {
        self.log.push(Ev::Overlay(state));
    }
    fn set_elapsed(&self, elapsed: Elapsed) {
        self.log.elapsed.lock().push(elapsed.clone());
        self.log.push(Ev::Tick(elapsed));
    }
    fn set_level(&self, level: f32) {
        // Levels stay out of the event stream: at 25 Hz they would swamp every
        // exact-sequence assertion in this file. `Log::levels()` is where a
        // test asks whether the meter was fed.
        self.log.levels.lock().push(level);
    }
    fn set_recording_indicator(&self, recording: bool) {
        self.log.push(Ev::Indicator(recording));
    }
    fn set_last_transcription(&self, text: &str) {
        self.log.push(Ev::LastTranscription(text.to_string()));
    }
    fn notify_changed(&self, topic: &str) {
        self.log.push(Ev::Notify(topic.to_string()));
    }
}

/// How `FakeCapture::start` fails. Two cases, because they produce different
/// overlay copy.
#[derive(Debug, Clone, Copy)]
enum StartFailure {
    Unavailable,
    PermissionDenied,
}

struct FakeCapture {
    log: Arc<Log>,
    /// Packets the next `stop()` hands back.
    packets: Mutex<Vec<Vec<i16>>>,
    outcome: Mutex<Result<StartOutcome, StartFailure>>,
    faults: Mutex<Vec<CaptureFault>>,
    recording: AtomicBool,
    /// How long opening the device takes.
    ///
    /// Zero for every test that does not care. A real capture stream takes
    /// tens of milliseconds to open and blocks the caller for all of them,
    /// which is precisely what makes "dispatch the slow work *after* the
    /// microphone is live" an observable claim rather than a stylistic one:
    /// anything started too early lands inside this window.
    start_latency: Mutex<Duration>,
    /// The meter sink the overlay installs, so a test can drive it directly.
    level_sink: Mutex<Option<wl_platform::audio::LevelSink>>,
    /// The live packet sink installed before capture is armed.
    packet_sink: Mutex<Option<wl_platform::audio::PacketSink>>,
}

impl FakeCapture {
    fn new(log: Arc<Log>, packet_count: usize) -> Self {
        Self {
            log,
            packets: Mutex::new(packets(packet_count)),
            outcome: Mutex::new(Ok(StartOutcome::Started)),
            faults: Mutex::new(Vec::new()),
            recording: AtomicBool::new(false),
            start_latency: Mutex::new(Duration::ZERO),
            level_sink: Mutex::new(None),
            packet_sink: Mutex::new(None),
        }
    }
}

impl AudioCapture for FakeCapture {
    fn list_devices(&self) -> wl_platform::Result<Vec<InputDevice>> {
        Ok(Vec::new())
    }
    fn prewarm(&self) -> wl_platform::Result<()> {
        self.log.push(Ev::CapturePrewarm);
        Ok(())
    }
    fn release(&self) -> wl_platform::Result<()> {
        self.log.push(Ev::CaptureRelease);
        Ok(())
    }
    fn start(&self) -> wl_platform::Result<StartOutcome> {
        let outcome = self.outcome.lock().clone();
        match outcome {
            Ok(outcome) => {
                let latency = *self.start_latency.lock();
                if !latency.is_zero() {
                    std::thread::sleep(latency);
                }
                self.recording.store(true, Ordering::SeqCst);
                self.log.push(Ev::CaptureStart);
                if let Some(sink) = self.packet_sink.lock().clone() {
                    for packet in self.packets.lock().iter() {
                        sink(packet);
                    }
                }
                Ok(outcome)
            }
            Err(StartFailure::Unavailable) => {
                Err(PlatformError::AudioDevice("no such device".into()))
            }
            Err(StartFailure::PermissionDenied) => Err(PlatformError::PermissionDenied(
                "microphone \u{2014} open ms-settings:privacy-microphone",
            )),
        }
    }
    fn stop(&self) -> Vec<Vec<i16>> {
        self.recording.store(false, Ordering::SeqCst);
        let packets = std::mem::take(&mut *self.packets.lock());
        // A real capture publishes one level per 40 ms packet from its worker
        // thread. Replaying them all at stop keeps the count honest — "the
        // meter stopped being fed" is a regression a silent fake would hide —
        // without needing a thread. The ramp makes the ordering assertable.
        let sink = self.level_sink.lock().clone();
        if let Some(sink) = sink {
            let total = packets.len().max(1) as f32;
            for index in 0..packets.len() {
                sink(index as f32 / total);
            }
        }
        self.log.push(Ev::CaptureStop(packets.len()));
        packets
    }
    fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }
    fn take_faults(&self) -> Vec<CaptureFault> {
        std::mem::take(&mut *self.faults.lock())
    }
    fn set_device(&self, _id: Option<&str>) -> wl_platform::Result<()> {
        Ok(())
    }
    fn set_level_sink(&self, sink: Option<wl_platform::audio::LevelSink>) {
        *self.level_sink.lock() = sink;
    }

    fn set_packet_sink(&self, sink: Option<wl_platform::audio::PacketSink>) {
        *self.packet_sink.lock() = sink;
    }
}

struct FakeSounds {
    log: Arc<Log>,
}

impl SoundPlayer for FakeSounds {
    fn play(&self, cue: Cue) {
        self.log.push(Ev::Cue(cue));
    }
    fn set_pack(&self, _pack: Option<&str>) -> wl_platform::Result<()> {
        Ok(())
    }
    fn available_packs(&self) -> Vec<String> {
        vec!["default".into()]
    }
    fn set_enabled(&self, _enabled: bool) {}
}

struct FakeHotkeys {
    tx: Sender<HotkeyEvent>,
    rx: Receiver<HotkeyEvent>,
    paused: AtomicBool,
}

impl FakeHotkeys {
    fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            tx,
            rx,
            paused: AtomicBool::new(false),
        }
    }
}

impl HotkeyBackend for FakeHotkeys {
    fn rebind(&self, _dictate: &[Hotkey]) -> wl_platform::Result<()> {
        Ok(())
    }
    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }
    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
    fn events(&self) -> Receiver<HotkeyEvent> {
        self.rx.clone()
    }
    fn reset(&self) {}
    fn is_healthy(&self) -> bool {
        true
    }
    fn begin_capture(&self) {}
    fn end_capture(&self) -> Option<Hotkey> {
        None
    }
}

/// The clipboard the fake injector shares with the pipeline.
const INITIAL_CLIPBOARD: &str = "original clipboard";

struct FakeInjector {
    log: Arc<Log>,
    modes: Mutex<Vec<InjectMode>>,
    clipboard: Mutex<String>,
    /// Every value handed back to `restore_clipboard`, in order.
    restored: Mutex<Vec<String>>,
}

impl TextInjector for FakeInjector {
    fn inject(&self, text: &str, mode: InjectMode) -> wl_platform::Result<()> {
        self.modes.lock().push(mode);
        self.log.push(Ev::Inject(text.to_string()));
        Ok(())
    }
    fn cancel_typing(&self) {
        self.log.push(Ev::CancelTyping);
    }
    fn undo_last_injection(&self) -> wl_platform::Result<()> {
        self.log.push(Ev::Undo);
        Ok(())
    }
    fn read_focused_text(&self) -> Vec<String> {
        self.log.push(Ev::ReadFocused);
        vec!["focused field".into()]
    }
    fn snapshot_clipboard(&self) -> wl_platform::Result<ClipboardSnapshot> {
        self.log.push(Ev::ClipboardSnapshot);
        Ok(ClipboardSnapshot(Box::new(self.clipboard.lock().clone())))
    }
    fn restore_clipboard(&self, snapshot: ClipboardSnapshot) -> wl_platform::Result<()> {
        let text = *snapshot
            .0
            .downcast::<String>()
            .expect("the pipeline hands back the snapshot it was given");
        self.clipboard.lock().clone_from(&text);
        self.restored.lock().push(text);
        self.log.push(Ev::ClipboardRestore);
        Ok(())
    }
}

struct FakeForeground {
    log: Arc<Log>,
    app: RwLock<AppInfo>,
}

impl ForegroundApp for FakeForeground {
    fn current(&self) -> AppInfo {
        let app = self.app.read().clone();
        self.log.push(Ev::ForegroundRead(app.name.clone()));
        app
    }
}

struct FakeScreen {
    log: Arc<Log>,
}

impl ScreenText for FakeScreen {
    fn ocr_frontmost_window(&self, _max_lines: usize) -> Vec<String> {
        self.log.push(Ev::Ocr);
        vec!["on screen".into()]
    }
}

struct FakeMedia {
    log: Arc<Log>,
}

impl MediaControl for FakeMedia {
    fn pause(&self) -> bool {
        self.log.push(Ev::MusicPause);
        true
    }
    fn resume(&self) {
        self.log.push(Ev::MusicResume);
    }
}

struct FakePermissions;

impl Permissions for FakePermissions {
    fn status(&self, _permission: Permission) -> PermissionState {
        PermissionState::Granted
    }
    fn request(&self, _permission: Permission) {}
    fn open_settings(&self, _permission: Permission) {}
}

/// A provider that replays a script: one entry consumed per attempt, the last
/// entry repeating once the script runs out.
struct LoopbackProvider {
    log: Arc<Log>,
    script: Mutex<Vec<Result<TranscriptResult, ProviderError>>>,
    attempts: AtomicUsize,
    /// Blocks each attempt, so the processing deadline can be exercised.
    delay: Mutex<Duration>,
    last_context: Arc<Mutex<Option<DictationContext>>>,
    last_fed_count: Arc<AtomicUsize>,
    ready: AtomicBool,
}

impl LoopbackProvider {
    fn new(log: Arc<Log>, script: Vec<Result<TranscriptResult, ProviderError>>) -> Self {
        Self {
            log,
            script: Mutex::new(script),
            attempts: AtomicUsize::new(0),
            delay: Mutex::new(Duration::ZERO),
            last_context: Arc::new(Mutex::new(None)),
            last_fed_count: Arc::new(AtomicUsize::new(0)),
            ready: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl TranscriptionProvider for LoopbackProvider {
    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
    async fn health(&self) -> Result<(), ProviderError> {
        Ok(())
    }
    async fn start(
        &self,
        _ctx: &DictationContext,
    ) -> Result<Box<dyn DictationSession>, ProviderError> {
        self.log.push(Ev::ProviderStart);
        Ok(Box::new(LoopbackSession {
            log: self.log.clone(),
            script: self.script.lock().clone(),
            attempt: self.attempts.fetch_add(1, Ordering::SeqCst),
            delay: *self.delay.lock(),
            last_context: self.last_context.clone(),
            last_fed_count: self.last_fed_count.clone(),
            fed: Mutex::new(Vec::new()),
        }))
    }
    fn reset(&self) {
        self.log.push(Ev::ProviderReset);
    }
}

/// One scripted attempt. Records the audio it was fed and the context it was
/// finished with, so tests can assert that a retry replays the same packets and
/// that context is sampled at the press rather than at injection.
struct LoopbackSession {
    log: Arc<Log>,
    script: Vec<Result<TranscriptResult, ProviderError>>,
    attempt: usize,
    delay: Duration,
    last_context: Arc<Mutex<Option<DictationContext>>>,
    last_fed_count: Arc<AtomicUsize>,
    fed: Mutex<Vec<Vec<i16>>>,
}

#[async_trait]
impl DictationSession for LoopbackSession {
    fn feed(&self, packet: &[i16]) {
        self.fed.lock().push(packet.to_vec());
    }

    async fn finish(
        self: Box<Self>,
        ctx: &DictationContext,
    ) -> Result<TranscriptResult, ProviderError> {
        self.log.push(Ev::Transcribe);
        *self.last_context.lock() = Some(ctx.clone());
        self.last_fed_count
            .store(self.fed.lock().len(), Ordering::SeqCst);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.script
            .get(self.attempt)
            .or_else(|| self.script.last())
            .cloned()
            .unwrap_or(Err(ProviderError::EmptyResult))
    }

    fn cancel(self: Box<Self>) {}
}

// Harness
// ---------------------------------------------------------------------------

fn packets(n: usize) -> Vec<Vec<i16>> {
    (0..n).map(|i| vec![i as i16; CHUNK_SAMPLES]).collect()
}

fn transcript(asr: &str, formatted: Option<&str>) -> TranscriptResult {
    TranscriptResult {
        id: "TRANSCRIPT-1".into(),
        asr_text: Some(asr.into()),
        formatted_text: formatted.map(str::to_string),
        duration_secs: 1.2,
        num_words: wl_core::text::word_count(formatted.unwrap_or(asr)),
    }
}

fn fast_timings() -> Timings {
    Timings {
        retry_delay: Duration::from_millis(5),
        rearm_debounce: Duration::from_millis(5),
        fault_poll: Duration::from_millis(5),
        tick: Duration::from_millis(10),
        processing_timeout_base: Duration::from_secs(30),
    }
}

struct Harness {
    pipeline: Arc<Pipeline>,
    log: Arc<Log>,
    hotkeys: Arc<FakeHotkeys>,
    capture: Arc<FakeCapture>,
    injector: Arc<FakeInjector>,
    provider: Arc<LoopbackProvider>,
    foreground: Arc<FakeForeground>,
    settings: Arc<RwLock<Settings>>,
    spool: Arc<Spool>,
    history: Arc<HistoryStore>,
    dictionary: Arc<DictionaryStore>,
    /// Shared with the pipeline, so a test can stand a second store on it and
    /// watch what the pipeline's own caches did or did not notice.
    db: Arc<Database>,
    db_path: PathBuf,
    downloads: PathBuf,
    _dir: tempfile::TempDir,
}

/// Everything a test may want to vary before the pipeline starts.
struct Builder {
    packets: usize,
    script: Vec<Result<TranscriptResult, ProviderError>>,
    settings: Settings,
    timings: Timings,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            packets: 10,
            script: vec![Ok(transcript("hello world", Some("Hello, world.")))],
            settings: Settings::default(),
            timings: fast_timings(),
        }
    }
}

impl Builder {
    fn build(self) -> Harness {
        let log = Arc::new(Log::default());
        let dir = tempfile::tempdir().expect("tempdir");
        let downloads = dir.path().join("Downloads");
        std::fs::create_dir_all(&downloads).expect("downloads dir");

        let db_path = dir.path().join("lightning.db");
        let db = Arc::new(Database::open_at(&db_path).expect("open db"));
        let history = Arc::new(HistoryStore::new(db.clone()));
        let dictionary = Arc::new(DictionaryStore::new(db.clone()));

        let capture = Arc::new(FakeCapture::new(log.clone(), self.packets));
        let hotkeys = Arc::new(FakeHotkeys::new());
        let injector = Arc::new(FakeInjector {
            log: log.clone(),
            modes: Mutex::new(Vec::new()),
            clipboard: Mutex::new(INITIAL_CLIPBOARD.to_string()),
            restored: Mutex::new(Vec::new()),
        });
        let foreground = Arc::new(FakeForeground {
            log: log.clone(),
            app: RwLock::new(AppInfo {
                name: "Notes".into(),
                bundle_id: "com.apple.Notes".into(),
                kind: AppKind::Other,
                url: String::new(),
            }),
        });
        let provider = Arc::new(LoopbackProvider::new(log.clone(), self.script));
        let settings = Arc::new(RwLock::new(self.settings));
        let spool = Arc::new(Spool::new(dir.path().join("PendingAudio")));

        let pipeline = Pipeline::spawn(PipelineDeps {
            settings: settings.clone(),
            platform: Platform {
                foreground: foreground.clone(),
                injector: injector.clone(),
                screen: Arc::new(FakeScreen { log: log.clone() }),
                media: Arc::new(FakeMedia { log: log.clone() }),
                permissions: Arc::new(FakePermissions),
            },
            audio: capture.clone(),
            sound: Arc::new(FakeSounds { log: log.clone() }),
            hotkeys: hotkeys.clone(),
            provider: Arc::new(RwLock::new(
                provider.clone() as Arc<dyn TranscriptionProvider>
            )),
            history: history.clone(),
            dictionary: dictionary.clone(),
            spool: spool.clone(),
            ui: Arc::new(RecordingUi { log: log.clone() }),
            downloads_dir: downloads.clone(),
            timings: self.timings,
        });

        Harness {
            pipeline,
            log,
            hotkeys,
            capture,
            injector,
            provider,
            foreground,
            settings,
            spool,
            history,
            dictionary,
            db,
            db_path,
            downloads,
            _dir: dir,
        }
    }
}

impl Harness {
    fn press(&self) {
        self.hotkeys
            .tx
            .send(HotkeyEvent {
                binding: Binding::Dictate,
                transition: Transition::Pressed,
            })
            .expect("send press");
    }

    fn release(&self) {
        self.hotkeys
            .tx
            .send(HotkeyEvent {
                binding: Binding::Dictate,
                transition: Transition::Released,
            })
            .expect("send release");
    }

    /// The chord guard cancelling a hold: the backend saw a keystroke land
    /// under a held bare modifier, so the user was reaching for a shortcut.
    fn chord_abort(&self) {
        self.hotkeys
            .tx
            .send(HotkeyEvent {
                binding: Binding::Dictate,
                transition: Transition::Aborted,
            })
            .expect("send chord abort");
    }

    /// A push-to-talk hold long enough to skip the tap-lock window, followed by
    /// the trailing-buffer stop.
    async fn hold_and_release(&self) {
        self.press();
        wait_for("recording to start", || {
            self.log.contains(&Ev::CaptureStart)
        })
        .await;
        // Beyond LOCK_DEBOUNCE the FSM treats this as a genuine hold, so the
        // stop lands one TRAILING_BUFFER later.
        tokio::time::sleep(wl_core::fsm::LOCK_DEBOUNCE + Duration::from_millis(20)).await;
        self.release();
    }

    fn spooled_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.spool.dir()) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pcm"))
            .collect()
    }

    /// Block until the recording has actually reached disk. The spool write is
    /// a background task, so "no files yet" and "file already deleted" look
    /// identical from outside; every spool assertion has to start here.
    async fn await_spooled(&self) {
        wait_for("the recording to be spooled", || {
            self.spooled_files().len() == 1
        })
        .await;
    }
}

fn core_event(event: &Ev) -> bool {
    matches!(
        event,
        Ev::Cue(_)
            | Ev::CaptureStart
            | Ev::CaptureStop(_)
            | Ev::Inject(_)
            | Ev::Overlay(_)
            | Ev::Indicator(_)
    )
}

// ---------------------------------------------------------------------------
// Push-to-talk
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unconfigured_vendor_refuses_before_the_microphone_opens() {
    let h = Builder::default().build();
    h.provider.ready.store(false, Ordering::SeqCst);

    // Not `hold_and_release`: that waits for the recording to start, which is
    // precisely what must not happen here. Drive the keys directly.
    h.press();
    h.release();

    wait_for("the error to surface", || {
        h.log
            .snapshot()
            .iter()
            .any(|e| matches!(e, Ev::Overlay(OverlayState::Error { .. })))
    })
    .await;

    // The point of the gate: the user is told before they speak, so nothing
    // was captured and nothing was sent. A refusal that still opened the mic
    // would be indistinguishable from the failure it replaces.
    assert!(
        !h.log.contains(&Ev::CaptureStart),
        "the microphone must not open for a vendor that cannot transcribe"
    );
    assert!(
        !h.log.contains(&Ev::Transcribe),
        "nothing may be sent to an unconfigured vendor"
    );
    assert!(
        !h.log.contains(&Ev::Cue(Cue::Start)),
        "a start cue would promise a recording that is not happening"
    );

    // And the refusal must not wedge the machine: a vendor configured a moment
    // later has to work on the very next press, with no restart.
    h.provider.ready.store(true, Ordering::SeqCst);
    h.hold_and_release().await;
    wait_for("the recovered dictation to inject", || {
        h.log.contains(&Ev::Inject("Hello, world.".into()))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_push_to_talk_hold_runs_the_whole_dictation_sequence() {
    let h = Builder::default().build();
    h.hold_and_release().await;

    wait_for("the overlay to hide", || {
        h.log.contains(&Ev::Overlay(OverlayState::Hidden))
    })
    .await;

    assert_eq!(
        h.log.sequence(core_event),
        vec![
            Ev::Cue(Cue::Start),
            Ev::CaptureStart,
            Ev::Indicator(true),
            Ev::Overlay(OverlayState::Recording),
            Ev::CaptureStop(10),
            Ev::Cue(Cue::Stop),
            Ev::Indicator(false),
            Ev::Overlay(OverlayState::Processing),
            Ev::Overlay(OverlayState::Inserting),
            Ev::Inject("Hello, world.".into()),
            Ev::Overlay(OverlayState::Hidden),
        ]
    );
}

/// HTK-040. The press sequence is an order, not a set.
///
/// The app sample has to come first — it is the frontmost app *at the instant
/// of the press*, and every later step either takes time or moves focus. The
/// cue has to precede the microphone opening, because the user needs to hear
/// that the app heard them before the first sample matters. And the four
/// concurrent jobs have to be dispatched after the microphone is live, or the
/// user's first syllable is spent on an AppleScript round trip.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_press_sequence_runs_in_a_fixed_order() {
    let settings = Settings {
        mute_music: true,
        use_accessibility_context: true,
        use_screen_context: true,
        ..Default::default()
    };
    let tick = Duration::from_millis(500);
    let h = Builder {
        settings,
        // A tick slow enough that the timer cannot possibly fire inside the
        // press sequence, so "the timer is armed last" is observable rather
        // than a race with the assertion below.
        timings: Timings {
            tick,
            ..fast_timings()
        },
        ..Builder::default()
    }
    .build();
    // Opening the device takes real time, so anything dispatched ahead of it
    // lands inside that window and is caught by the index comparison below.
    // Without this the assertion is vacuous: a `spawn_blocking` issued one
    // line too early still logs after an instant `start()`.
    *h.capture.start_latency.lock() = Duration::from_millis(150);

    h.press();
    // The elapsed reset is the last thing the actor does inline.
    wait_for("the press sequence to finish", || {
        h.log.contains(&Ev::Tick(Elapsed::default()))
    })
    .await;

    assert_eq!(
        h.log.sequence(|e| matches!(
            e,
            Ev::ForegroundRead(_)
                | Ev::Cue(_)
                | Ev::CaptureStart
                | Ev::Indicator(_)
                | Ev::Overlay(_)
                | Ev::Tick(_)
        )),
        vec![
            Ev::ForegroundRead("Notes".into()),
            Ev::Cue(Cue::Start),
            Ev::CaptureStart,
            Ev::Indicator(true),
            Ev::Overlay(OverlayState::Recording),
            Ev::Tick(Elapsed::default()),
        ]
    );

    // Music, the live provider stream, accessibility and OCR run after the
    // microphone is armed. Their relative order is scheduler-dependent.
    wait_for("the concurrent work to be dispatched", || {
        h.log.contains(&Ev::MusicPause)
            && h.log.contains(&Ev::ProviderStart)
            && h.log.contains(&Ev::ReadFocused)
            && h.log.contains(&Ev::Ocr)
    })
    .await;
    let opened = h.log.index_of(&Ev::CaptureStart).expect("the microphone");
    for spawned in [Ev::MusicPause, Ev::ProviderStart, Ev::ReadFocused, Ev::Ocr] {
        let at = h.log.index_of(&spawned).expect("dispatched");
        assert!(
            at > opened,
            "{spawned:?} ran at {at} but the microphone only opened at {opened}; \
             nothing slow may sit between the key press and the first sample"
        );
    }

    // And only now does the 1 Hz readout begin.
    wait_for("the first tick", || {
        h.log.count(&Ev::Tick(Elapsed::default())) == 2
    })
    .await;
}

/// HTK-041. The stop sequence is likewise fixed, and the minimum-length gate is
/// the last step rather than the first: the cue and the tray have to reflect
/// "you stopped" even for a take that is about to be thrown away.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_stop_sequence_runs_in_a_fixed_order() {
    let h = Builder::default().build();
    // Park the provider so the sequence under test ends where the stop
    // sequence does, rather than running on into the injection.
    *h.provider.delay.lock() = Duration::from_secs(30);
    h.hold_and_release().await;

    wait_for("the recording to be handed on", || {
        h.log.contains(&Ev::Overlay(OverlayState::Processing))
    })
    .await;
    settle().await;

    let events = h.log.snapshot();
    let stop = events
        .iter()
        .position(|e| *e == Ev::CaptureStop(10))
        .expect("the microphone closed");
    let tail: Vec<Ev> = events[stop..]
        .iter()
        .filter(|e| {
            matches!(
                e,
                Ev::CaptureStop(_) | Ev::Cue(_) | Ev::Indicator(_) | Ev::Overlay(_)
            )
        })
        .cloned()
        .collect();
    assert_eq!(
        tail,
        vec![
            Ev::CaptureStop(10),
            Ev::Cue(Cue::Stop),
            Ev::Indicator(false),
            Ev::Overlay(OverlayState::Processing),
        ]
    );
    assert!(
        !events[stop..].iter().any(|e| matches!(e, Ev::Tick(_))),
        "the elapsed timer is invalidated before the microphone is closed, so \
         no readout may outlive the recording"
    );

    // The state reached idle before any of that: a press now is a new
    // recording, not a second stop.
    h.press();
    wait_for("a fresh recording", || h.log.count(&Ev::CaptureStart) == 2).await;
}

/// HTK-041, the other side of the gate. A take under [`MIN_PACKETS`] still gets
/// its stop cue and still turns the tray off; only the hand-off is skipped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_minimum_length_gate_runs_after_the_cue_and_the_indicator() {
    let h = Builder {
        packets: MIN_PACKETS - 1,
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("the overlay to hide", || {
        h.log.contains(&Ev::Overlay(OverlayState::Hidden))
    })
    .await;
    settle().await;

    let events = h.log.snapshot();
    let stop = events
        .iter()
        .position(|e| *e == Ev::CaptureStop(MIN_PACKETS - 1))
        .expect("the microphone closed");
    let tail: Vec<Ev> = events[stop..]
        .iter()
        .filter(|e| {
            matches!(
                e,
                Ev::CaptureStop(_) | Ev::Cue(_) | Ev::Indicator(_) | Ev::Overlay(_)
            )
        })
        .cloned()
        .collect();
    assert_eq!(
        tail,
        vec![
            Ev::CaptureStop(MIN_PACKETS - 1),
            Ev::Cue(Cue::Stop),
            Ev::Indicator(false),
            Ev::Overlay(OverlayState::Hidden),
        ],
        "the gate is the last step of the stop sequence, not the first"
    );
}

/// The production WebSocket session opens while the user is still speaking,
/// and the same session is finalized after capture stops.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_provider_stream_starts_at_the_press_and_receives_live_audio() {
    let h = Builder::default().build();
    h.press();

    wait_for("the live provider stream", || {
        h.log.contains(&Ev::ProviderStart)
    })
    .await;
    assert!(
        !h.log
            .snapshot()
            .iter()
            .any(|e| matches!(e, Ev::CaptureStop(_))),
        "the provider must open before capture stops"
    );

    tokio::time::sleep(wl_core::fsm::LOCK_DEBOUNCE + Duration::from_millis(20)).await;
    h.release();
    wait_for("the transcription", || h.log.contains(&Ev::Transcribe)).await;

    let start = h.log.index_of(&Ev::ProviderStart).expect("a live stream");
    let stop = h.log.index_of(&Ev::CaptureStop(10)).expect("a stop");
    let finish = h.log.index_of(&Ev::Transcribe).expect("a finalization");
    assert!(start < stop && stop < finish);
    assert_eq!(
        h.provider.last_fed_count.load(Ordering::SeqCst),
        10,
        "every captured packet must reach the live provider session before finalization"
    );
}

/// CTX-009. The frontmost app is sampled once, at the press, and never again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_frontmost_app_is_sampled_at_the_press_not_at_injection() {
    let h = Builder::default().build();
    h.press();
    wait_for("recording to start", || h.log.contains(&Ev::CaptureStart)).await;

    // The user tabs away mid-dictation. The transcript must still be filed
    // against the app they were actually dictating into.
    *h.foreground.app.write() = AppInfo {
        name: "Terminal".into(),
        bundle_id: "com.apple.Terminal".into(),
        kind: AppKind::Other,
        url: String::new(),
    };

    tokio::time::sleep(wl_core::fsm::LOCK_DEBOUNCE + Duration::from_millis(20)).await;
    h.release();
    wait_for("a history row", || {
        h.history.entries(10, 0).expect("entries").len() == 1
    })
    .await;
    settle().await;

    let entries = h.history.entries(10, 0).expect("entries");
    assert_eq!(entries[0].app_name, "Notes");
    assert_eq!(
        h.provider
            .last_context
            .lock()
            .as_ref()
            .expect("a request")
            .app
            .name,
        "Notes"
    );
    // The values above would also be right if the app were re-read at
    // injection time and simply happened to agree. This is the assertion that
    // says it is never re-read: exactly one sample, taken at the press.
    assert_eq!(
        h.log
            .sequence(|e| matches!(e, Ev::ForegroundRead(_) | Ev::CaptureStart)),
        vec![Ev::ForegroundRead("Notes".into()), Ev::CaptureStart]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_press_inside_the_debounce_locks_hands_free_mode() {
    let h = Builder::default().build();
    h.press();
    wait_for("recording to start", || h.log.contains(&Ev::CaptureStart)).await;
    h.release();
    h.press();

    wait_for("the locked overlay", || {
        h.log.contains(&Ev::Overlay(OverlayState::Locked))
    })
    .await;

    // A release in locked mode is meaningless; only the next press stops.
    h.release();
    settle().await;
    assert!(
        !h.log.contains(&Ev::CaptureStop(10)),
        "a release must not stop a locked recording"
    );

    h.press();
    wait_for("the recording to stop", || {
        h.log.contains(&Ev::CaptureStop(10))
    })
    .await;
}

/// The chord guard's payoff at the pipeline level: a `Ctrl+C` typed under a
/// held bare Control throws the take away instead of transcribing half a
/// second of room noise into whatever the user was copying from.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chord_abort_discards_a_push_to_talk_take() {
    let h = Builder::default().build();
    h.press();
    wait_for("recording to start", || h.log.contains(&Ev::CaptureStart)).await;

    h.chord_abort();
    wait_for("the overlay to hide", || {
        h.log.contains(&Ev::Overlay(OverlayState::Hidden))
    })
    .await;
    settle().await;

    assert!(
        !h.log.contains(&Ev::Transcribe),
        "an abandoned hold must never reach the provider"
    );
    assert!(h.log.injected().is_empty(), "and never reach the caret");

    // Idle again, so the next press is a start rather than a stop.
    h.press();
    wait_for("a fresh recording", || h.log.count(&Ev::CaptureStart) == 2).await;
}

/// Hands-free is the case the guard must not touch. The modifier is not held
/// there, so a `Ctrl+C` is an ordinary shortcut — and the backend can still
/// see a held modifier for the instant of the locking press itself, which is
/// exactly what the `State::Listening` gate in `on_hotkey` is for. Without it
/// this test loses its recording.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chord_abort_leaves_hands_free_recording_alone() {
    let h = Builder::default().build();
    h.press();
    wait_for("recording to start", || h.log.contains(&Ev::CaptureStart)).await;
    h.release();
    h.press();
    wait_for("the locked overlay", || {
        h.log.contains(&Ev::Overlay(OverlayState::Locked))
    })
    .await;

    h.chord_abort();
    settle().await;
    assert!(
        !h.log.contains(&Ev::Overlay(OverlayState::Hidden)),
        "a locked recording must survive a chord abort"
    );

    // Still recording, and still stoppable the normal way.
    h.press();
    wait_for("the recording to stop", || {
        h.log.contains(&Ev::CaptureStop(10))
    })
    .await;
    wait_for("a transcription", || h.log.contains(&Ev::Transcribe)).await;
}

/// AUD-032. A microphone that will not open unwinds in a fixed order and stops
/// dead: overlay error, state back to idle, music released, and the provider
/// never hears about a recording that does not exist.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_microphone_shows_mic_unavailable_and_returns_to_idle() {
    let settings = Settings {
        mute_music: true,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();
    *h.capture.outcome.lock() = Err(StartFailure::Unavailable);
    h.press();

    wait_for("music to resume", || h.log.contains(&Ev::MusicResume)).await;
    settle().await;

    assert_eq!(
        h.log.snapshot(),
        vec![
            Ev::ForegroundRead("Notes".into()),
            Ev::Cue(Cue::Start),
            Ev::Overlay(OverlayState::Error {
                message: "Mic unavailable".into(),
            }),
            Ev::MusicResume,
        ],
        "a recording that never started must not light the tray, must not show \
         the recording overlay, must not pause music it will not be listening \
         over, and must not tick"
    );
    assert!(
        !h.log.contains(&Ev::ProviderStart),
        "there is nothing to transcribe, so no provider stream may open"
    );
    assert!(!h.log.contains(&Ev::Transcribe));

    // Back to idle: the next press is a fresh start, not a stop.
    *h.capture.outcome.lock() = Ok(StartOutcome::Started);
    h.press();
    wait_for("a fresh start", || h.log.contains(&Ev::CaptureStart)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fallback_microphone_still_records() {
    let h = Builder::default().build();
    *h.capture.outcome.lock() = Ok(StartOutcome::StartedWithFallback {
        requested: "Yeti".into(),
    });
    h.hold_and_release().await;

    wait_for("the transcript", || h.log.contains(&Ev::Transcribe)).await;
    assert!(h.log.contains(&Ev::Overlay(OverlayState::Recording)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_denied_microphone_shows_the_platform_hint_verbatim() {
    let h = Builder::default().build();
    *h.capture.outcome.lock() = Err(StartFailure::PermissionDenied);
    h.press();

    wait_for("the permission message", || {
        h.log.overlays().iter().any(|state| {
            matches!(state, OverlayState::Error { message }
                if message.contains("ms-settings:privacy-microphone"))
        })
    })
    .await;
    assert!(
        !h.log.contains(&Ev::Overlay(OverlayState::Error {
            message: "Mic unavailable".into(),
        })),
        "the generic copy would hide the one thing the user needs to know"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launch_seeds_the_dictionary_and_applies_the_configured_microphone() {
    let settings = Settings {
        keep_microphone_active: true,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();

    wait_for("the seeded phrase", || learned(&h, "Wispr Lightning")).await;
    wait_for("the microphone to be held open", || {
        h.log.contains(&Ev::CapturePrewarm)
    })
    .await;
}

/// AUD-024 and LIF-005, both branches. Holding the microphone open keeps the
/// OS recording indicator lit, so it is strictly opt-in — a launch that
/// pre-warms an unwilling user's microphone is a privacy bug, not a latency
/// win.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_microphone_is_pre_warmed_at_launch_only_when_the_user_asked() {
    let hot = Builder {
        settings: Settings {
            keep_microphone_active: true,
            ..Default::default()
        },
        ..Builder::default()
    }
    .build();
    wait_for("the microphone to be held open", || {
        hot.log.contains(&Ev::CapturePrewarm)
    })
    .await;

    let cold = Builder::default().build();
    assert!(
        !cold.settings.read().keep_microphone_active,
        "the default is off, which is what makes the negative branch the one \
         that matters"
    );
    settle().await;
    assert!(
        !cold.log.contains(&Ev::CapturePrewarm),
        "launching must not open the microphone the user did not ask us to hold"
    );
    assert!(!cold.capture.is_recording());
}

/// LIF-006, the orchestrator's half: nothing is put on the overlay at launch.
///
/// The window itself is built hidden by `Overlay::create`; what this proves is
/// that no launch-path code shows it, so the first thing it is ever told is the
/// first press.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launch_never_puts_anything_on_the_overlay() {
    let h = Builder::default().build();
    settle().await;

    assert!(
        h.log.overlays().is_empty(),
        "the overlay is constructed at launch and shown at the first press; \
         anything on screen before that is an overlay the user cannot explain"
    );
    assert!(!h.log.contains(&Ev::Indicator(true)));

    h.press();
    wait_for("the recording overlay", || {
        h.log.contains(&Ev::Overlay(OverlayState::Recording))
    })
    .await;
    assert_eq!(h.log.overlays(), vec![OverlayState::Recording]);
}

/// DIC-008. Defaults are seeded, and then all three caches are primed, before
/// the first dictation.
///
/// "Primed" is proved by what the store cannot see: rows written straight to
/// the table behind its back are invisible to a warm cache and would be picked
/// up instantly by a cold one. That the seeded phrases *are* visible while the
/// later rows are not also fixes the order — a warm-up that ran before the seed
/// would have cached an empty vocabulary.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launch_seeds_the_dictionary_and_primes_all_three_caches() {
    let h = Builder::default().build();

    // Read straight from the table, so waiting for the seed does not itself
    // prime the cache under test.
    let seeded = || {
        let conn = rusqlite::Connection::open(&h.db_path).expect("open");
        let mut statement = conn
            .prepare("SELECT phrase FROM dictionary WHERE is_deleted = 0")
            .expect("prepare");
        let phrases: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("phrases");
        phrases
    };
    wait_for("the seeded phrase", || {
        seeded().iter().any(|phrase| phrase == "Wispr Lightning")
    })
    .await;
    // The warm-up is the next statement after the seed on the same task.
    settle().await;

    // Three rows the primed caches were never told about, one per cache.
    let behind_its_back = DictionaryStore::new(h.db.clone());
    behind_its_back
        .add_manual("Bergamot", None, false)
        .expect("a vocabulary row");
    behind_its_back
        .add_manual("gonna", Some("going to"), false)
        .expect("a replacement row");
    behind_its_back
        .add_manual("addr", Some("221B Baker Street"), true)
        .expect("a snippet row");

    assert!(
        learned(&h, "Wispr Lightning"),
        "the default was seeded before the caches were primed"
    );
    assert!(
        !learned(&h, "Bergamot"),
        "the vocabulary cache was cold: the first dictation of the session is \
         paying for the query DIC-008 says launch already paid"
    );
    assert!(
        h.dictionary
            .replacements()
            .expect("replacements")
            .is_empty(),
        "the replacement cache was cold"
    );
    assert!(
        h.dictionary.snippets().expect("snippets").is_empty(),
        "the snippet cache was cold"
    );
}

// ---------------------------------------------------------------------------
// The minimum-length gate
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recording_shorter_than_the_minimum_never_reaches_the_provider() {
    let h = Builder {
        packets: MIN_PACKETS - 1,
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("the overlay to hide", || {
        h.log.contains(&Ev::Overlay(OverlayState::Hidden))
    })
    .await;
    settle().await;

    assert!(
        !h.log.contains(&Ev::Transcribe),
        "a sub-minimum recording must not cost a network round trip"
    );
    assert!(h.log.contains(&Ev::ProviderReset));
    assert!(h.log.contains(&Ev::MusicResume));
    assert!(
        h.spooled_files().is_empty(),
        "a discarded recording must not be spooled"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_packets_over_a_second_reports_a_dead_microphone() {
    let h = Builder {
        packets: 0,
        ..Builder::default()
    }
    .build();

    h.press();
    wait_for("recording to start", || h.log.contains(&Ev::CaptureStart)).await;
    // Longer than DEAD_MIC_THRESHOLD, so this is a mic that went away rather
    // than a mis-press.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    h.release();

    wait_for("the dead-mic message", || {
        h.log.contains(&Ev::Overlay(OverlayState::Error {
            message: "Mic not responding".into(),
        }))
    })
    .await;
    assert!(!h.log.contains(&Ev::Transcribe));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_zero_packet_mis_press_is_silently_ignored() {
    let h = Builder {
        packets: 0,
        ..Builder::default()
    }
    .build();
    // A quick tap: the FSM stops it 0.5 s after the press, comfortably inside
    // DEAD_MIC_THRESHOLD, so this is a mis-press and not a dead microphone.
    h.press();
    h.release();

    wait_for("the overlay to hide", || {
        h.log.contains(&Ev::Overlay(OverlayState::Hidden))
    })
    .await;
    assert!(
        !h.log.contains(&Ev::Overlay(OverlayState::Error {
            message: "Mic not responding".into(),
        })),
        "a sub-second mis-press must not accuse the microphone"
    );
}

// ---------------------------------------------------------------------------
// Retries and failure
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retryable_error_is_retried_exactly_twice_then_becomes_recoverable() {
    let h = Builder {
        script: vec![
            Err(ProviderError::ConnectionFailed),
            Err(ProviderError::ConnectionFailed),
            Err(ProviderError::ConnectionFailed),
        ],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("the recoverable overlay", || {
        h.log.contains(&Ev::Overlay(OverlayState::Recoverable {
            message: ProviderError::ConnectionFailed.user_message(),
        }))
    })
    .await;
    settle().await;

    assert_eq!(
        h.log.count(&Ev::Transcribe),
        3,
        "one attempt plus exactly two automatic retries"
    );
    let overlays = h.log.overlays();
    assert!(overlays.contains(&OverlayState::Retrying { attempt: 2, of: 3 }));
    assert!(overlays.contains(&OverlayState::Retrying { attempt: 3, of: 3 }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_non_retryable_error_goes_recoverable_immediately() {
    let h = Builder {
        script: vec![Err(ProviderError::auth_failed())],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("the recoverable overlay", || {
        h.log.contains(&Ev::Overlay(OverlayState::Recoverable {
            message: ProviderError::auth_failed().user_message(),
        }))
    })
    .await;
    settle().await;

    assert_eq!(h.log.count(&Ev::Transcribe), 1);
    assert!(
        !h.log
            .overlays()
            .iter()
            .any(|s| matches!(s, OverlayState::Retrying { .. })),
        "a bad credential is not worth retrying"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retry_opens_a_fresh_stream_after_the_backoff() {
    let h = Builder {
        script: vec![
            Err(ProviderError::Timeout),
            Ok(transcript("second try", Some("Second try."))),
        ],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("the injected text", || {
        h.log.contains(&Ev::Inject("Second try.".into()))
    })
    .await;

    let events = h.log.snapshot();
    let retrying = events
        .iter()
        .position(|e| matches!(e, Ev::Overlay(OverlayState::Retrying { .. })))
        .expect("a retry was announced");
    let second_start = events
        .iter()
        .enumerate()
        .filter(|(_, e)| **e == Ev::ProviderStart)
        .nth(1)
        .expect("a fresh provider stream")
        .0;
    let second_finish = events
        .iter()
        .enumerate()
        .filter(|(_, e)| **e == Ev::Transcribe)
        .nth(1)
        .expect("a second attempt")
        .0;
    assert!(retrying < second_start && second_start < second_finish);
    assert_eq!(h.provider.attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_provider_trips_the_processing_deadline_and_keeps_the_audio() {
    let h = Builder {
        timings: Timings {
            processing_timeout_base: Duration::from_millis(30),
            ..fast_timings()
        },
        ..Builder::default()
    }
    .build();
    *h.provider.delay.lock() = Duration::from_secs(30);
    h.hold_and_release().await;

    wait_for("the timeout overlay", || {
        h.log.contains(&Ev::Overlay(OverlayState::Recoverable {
            message: "Timed out".into(),
        }))
    })
    .await;

    h.await_spooled().await;
    assert!(!h.log.contains(&Ev::Inject("Hello, world.".into())));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_processing_deadline_scales_with_the_recording() {
    let base = Duration::from_secs(30);
    assert_eq!(processing_timeout(base, 0.0), base);
    assert_eq!(processing_timeout(base, 120.0), Duration::from_secs(90));
    assert_eq!(processing_timeout(base, 600.0), Duration::from_secs(330));
}

// ---------------------------------------------------------------------------
// The spool
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_spooled_file_survives_a_failure_and_is_deleted_on_success() {
    let h = Builder {
        script: vec![Err(ProviderError::auth_failed())],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("the failure overlay", || {
        h.log.contains(&Ev::Overlay(OverlayState::Recoverable {
            message: ProviderError::auth_failed().user_message(),
        }))
    })
    .await;
    h.await_spooled().await;

    // Now let the retry succeed: the same audio should transcribe and the file
    // should go away.
    *h.provider.script.lock() = vec![Ok(transcript("recovered", Some("Recovered.")))];
    h.provider.attempts.store(0, Ordering::SeqCst);
    h.pipeline.overlay_action(OverlayAction::Retry);

    wait_for("the injected text", || {
        h.log.contains(&Ev::Inject("Recovered.".into()))
    })
    .await;
    wait_for("the spool to be cleaned up", || {
        h.spooled_files().is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saving_a_failed_recording_writes_a_wav_to_the_downloads_folder() {
    let h = Builder {
        script: vec![Err(ProviderError::auth_failed())],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;
    h.await_spooled().await;
    wait_for("the failure overlay", || {
        h.log
            .overlays()
            .iter()
            .any(|s| matches!(s, OverlayState::Recoverable { .. }))
    })
    .await;

    h.pipeline.overlay_action(OverlayAction::Save);
    wait_for("an exported wav", || {
        std::fs::read_dir(&h.downloads)
            .map(|d| d.count() > 0)
            .unwrap_or(false)
    })
    .await;

    let exported = std::fs::read_dir(&h.downloads)
        .expect("downloads")
        .next()
        .expect("a file")
        .expect("entry")
        .path();
    let bytes = std::fs::read(&exported).expect("read wav");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(
        h.spooled_files().len(),
        1,
        "exporting a copy must not consume the pending recording"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dismissing_a_failed_recording_discards_it() {
    let h = Builder {
        script: vec![Err(ProviderError::auth_failed())],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;
    wait_for("the failure overlay", || {
        h.log
            .overlays()
            .iter()
            .any(|s| matches!(s, OverlayState::Recoverable { .. }))
    })
    .await;

    h.pipeline.overlay_action(OverlayAction::Discard);
    wait_for("the overlay to hide", || {
        h.log.contains(&Ev::Overlay(OverlayState::Hidden))
    })
    .await;
    assert!(
        h.spooled_files().is_empty(),
        "dismissing means the audio is genuinely gone"
    );

    // Nothing pending: a stray Retry must not resurrect it.
    let attempts = h.log.count(&Ev::Transcribe);
    h.pipeline.overlay_action(OverlayAction::Retry);
    settle().await;
    assert_eq!(h.log.count(&Ev::Transcribe), attempts);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recovered_recording_is_offered_at_launch() {
    let h = Builder::default().build();
    let path = h.spool.save(&packets(12)).expect("spool");
    let recovered = h.spool.recover_latest().expect("recover");
    assert_eq!(recovered.path, path);

    h.pipeline.offer_recovery(recovered);
    wait_for("the recovery overlay", || {
        h.log.contains(&Ev::Overlay(OverlayState::Recoverable {
            message: "Recovered unsent recording".into(),
        }))
    })
    .await;

    // And it is genuinely retryable, with the app reported as unknown.
    h.pipeline.overlay_action(OverlayAction::Retry);
    wait_for("the injected text", || {
        h.log.contains(&Ev::Inject("Hello, world.".into()))
    })
    .await;
    wait_for("a history row", || {
        h.history.entries(10, 0).expect("entries").len() == 1
    })
    .await;
    assert_eq!(
        h.history.entries(10, 0).expect("entries")[0].app_name,
        "Unknown"
    );
}

// ---------------------------------------------------------------------------
// Text shaping
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_email_signature_is_appended_only_in_email_apps() {
    let settings = Settings {
        email_auto_signature: true,
        email_signature_option: EmailSignature::SpokenWithLightning,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();

    // First in a non-email app: no signature.
    h.hold_and_release().await;
    wait_for("the first injection", || !h.log.injected().is_empty()).await;
    assert_eq!(h.log.injected(), vec!["Hello, world.".to_string()]);

    // Then in an email client.
    *h.foreground.app.write() = AppInfo {
        name: "Mail".into(),
        bundle_id: "com.apple.mail".into(),
        kind: AppKind::Email,
        url: String::new(),
    };
    *h.capture.packets.lock() = packets(10);
    h.provider.attempts.store(0, Ordering::SeqCst);
    h.hold_and_release().await;

    wait_for("the second injection", || h.log.injected().len() == 2).await;
    assert_eq!(
        h.log.injected()[1],
        format!(
            "Hello, world.{}",
            EmailSignature::SpokenWithLightning.suffix()
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn natural_mode_selects_the_typing_injector() {
    let settings = Settings {
        natural_mode_enabled: true,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("the injection", || !h.log.injected().is_empty()).await;
    assert!(matches!(
        h.injector.modes.lock().first(),
        Some(InjectMode::Natural { .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_transcript_is_reported_and_nothing_is_injected() {
    let h = Builder {
        script: vec![Ok(TranscriptResult {
            id: "EMPTY".into(),
            asr_text: Some(String::new()),
            formatted_text: None,
            duration_secs: 0.4,
            num_words: 0,
        })],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("the empty-result message", || {
        h.log.contains(&Ev::Overlay(OverlayState::Error {
            message: ProviderError::EmptyResult.user_message(),
        }))
    })
    .await;
    assert!(h.log.injected().is_empty());
}

// ---------------------------------------------------------------------------
// History and auto-learn
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_successful_dictation_is_recorded_and_mines_new_words() {
    let settings = Settings {
        deepgram_language: "fr".into(),
        ..Default::default()
    };
    let h = Builder {
        settings,
        script: vec![Ok(transcript(
            "meeting with the team",
            Some("Meeting with the Kubernetes team"),
        ))],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("a history row", || {
        h.history.entries(10, 0).expect("entries").len() == 1
    })
    .await;
    let entry = h.history.entries(10, 0).expect("entries").remove(0);
    assert_eq!(
        entry.formatted_text.as_deref(),
        Some("Meeting with the Kubernetes team")
    );
    assert_eq!(entry.app_bundle_id, "com.apple.Notes");
    assert_eq!(entry.language, "fr");

    wait_for("the learned phrase", || {
        h.dictionary
            .vocabulary_phrases()
            .expect("vocabulary")
            .iter()
            .any(|p| p == "Kubernetes")
    })
    .await;
    assert!(h.log.contains(&Ev::Notify("history".into())));
}

/// Whether `phrase` made it into the dictionary. Not an emptiness check: the
/// pipeline seeds the user's name and the product name at launch.
fn learned(h: &Harness, phrase: &str) -> bool {
    h.dictionary
        .vocabulary_phrases()
        .expect("vocabulary")
        .iter()
        .any(|p| p == phrase)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_learn_is_skipped_when_the_user_turned_it_off() {
    let settings = Settings {
        auto_learn_words: false,
        ..Default::default()
    };
    let h = Builder {
        settings,
        script: vec![Ok(transcript(
            "meeting with the team",
            Some("Meeting with the Kubernetes team"),
        ))],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("a history row", || {
        h.history.entries(10, 0).expect("entries").len() == 1
    })
    .await;
    settle().await;
    assert!(
        !learned(&h, "Kubernetes"),
        "auto-learn must respect the setting"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_learn_needs_both_the_raw_and_the_formatted_text() {
    let h = Builder {
        script: vec![Ok(transcript("Meeting with Kubernetes", None))],
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("a history row", || {
        h.history.entries(10, 0).expect("entries").len() == 1
    })
    .await;
    settle().await;
    assert!(
        !learned(&h, "Kubernetes"),
        "with no formatted text there is no correction to learn from"
    );
}

// ---------------------------------------------------------------------------
// Deepgram context
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_reads_follow_their_settings() {
    let settings = Settings {
        use_accessibility_context: true,
        use_screen_context: true,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("a transcription", || h.log.contains(&Ev::Transcribe)).await;
    let request = h.provider.last_context.lock().clone().expect("a request");
    assert_eq!(request.ax_context, vec!["focused field".to_string()]);
    assert_eq!(request.ocr_context, vec!["on screen".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_context_is_never_read() {
    let settings = Settings {
        use_accessibility_context: false,
        use_screen_context: false,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("a transcription", || h.log.contains(&Ev::Transcribe)).await;
    assert!(!h.log.contains(&Ev::ReadFocused));
    assert!(!h.log.contains(&Ev::Ocr));
    let request = h.provider.last_context.lock().clone().expect("a request");
    assert!(request.ax_context.is_empty());
    assert!(request.ocr_context.is_empty());
}

// ---------------------------------------------------------------------------
// Devices, sleep and hotkey capture
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_fault_while_idle_rearms_the_microphone() {
    let settings = Settings {
        keep_microphone_active: true,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();

    // One pre-warm already happened at launch; the fault must produce another.
    assert_eq!(h.log.count(&Ev::CapturePrewarm), 1);
    h.capture.faults.lock().push(CaptureFault::DeviceLost);
    wait_for("the microphone to be re-armed", || {
        h.log.count(&Ev::CapturePrewarm) == 2
    })
    .await;
    assert!(h.log.contains(&Ev::CaptureRelease));
    assert!(h.log.contains(&Ev::Notify("devices".into())));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_fault_mid_recording_does_not_stop_the_recording() {
    let h = Builder::default().build();
    h.press();
    wait_for("recording to start", || h.log.contains(&Ev::CaptureStart)).await;

    h.capture.faults.lock().push(CaptureFault::DeviceLost);
    settle().await;
    assert!(
        !h.log.contains(&Ev::CaptureStop(10)),
        "the take must run to its natural end, as in the original"
    );
    assert!(!h.log.contains(&Ev::CapturePrewarm));

    tokio::time::sleep(wl_core::fsm::LOCK_DEBOUNCE + Duration::from_millis(20)).await;
    h.release();
    wait_for("the recording to stop", || {
        h.log.contains(&Ev::CaptureStop(10))
    })
    .await;

    // The next recording gets a fresh stream.
    *h.capture.packets.lock() = packets(10);
    h.press();
    wait_for("the stream to be rebuilt", || {
        h.log.contains(&Ev::CaptureRelease)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silent_input_gets_its_own_message() {
    let h = Builder::default().build();
    h.capture.faults.lock().push(CaptureFault::SilentInput);

    wait_for("the silence warning", || {
        h.log.overlays().iter().any(|s| {
            matches!(s, OverlayState::Error { message }
                if message.contains("microphone privacy"))
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_overrun_does_not_disturb_the_microphone() {
    let settings = Settings {
        keep_microphone_active: true,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();
    wait_for("the launch pre-warm", || {
        h.log.contains(&Ev::CapturePrewarm)
    })
    .await;

    h.capture.faults.lock().push(CaptureFault::Overrun);
    settle().await;

    assert_eq!(
        h.log.count(&Ev::CaptureRelease),
        0,
        "a dropped buffer says nothing about the device set"
    );
    assert_eq!(h.log.count(&Ev::CapturePrewarm), 1);
    assert!(!h.log.contains(&Ev::Notify("devices".into())));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sleep_abandons_an_in_flight_recording() {
    let h = Builder::default().build();
    h.press();
    wait_for("recording to start", || h.log.contains(&Ev::CaptureStart)).await;

    h.pipeline.abort();
    wait_for("the overlay to hide", || {
        h.log.contains(&Ev::Overlay(OverlayState::Hidden))
    })
    .await;
    settle().await;

    assert!(!h.log.contains(&Ev::Transcribe));
    assert!(h.log.contains(&Ev::Indicator(false)));
    assert!(h.log.contains(&Ev::MusicResume));

    // And the FSM is idle again: the next press starts a new recording.
    *h.capture.packets.lock() = packets(10);
    h.press();
    wait_for("a fresh recording", || h.log.count(&Ev::CaptureStart) == 2).await;
}

/// HTK-047 and the pipeline half of HTK-049.
///
/// Two separate promises: no press reaching the orchestrator while a binding is
/// being recorded may do *anything* observable, and a capture that comes back
/// empty must leave the stored binding exactly as it was. Binding a key that
/// silently starts a recording is the failure the user would notice; binding a
/// key that silently erases their old one is the failure they would not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hotkey_events_are_ignored_while_a_binding_is_being_captured() {
    let h = Builder::default().build();
    let bound = h.settings.read().hotkeys.clone();
    h.pipeline.set_capturing_hotkey(true);

    // Tap, hold, and the second press that would otherwise arm hands-free mode
    // must all be ignored while capture owns global input.
    h.press();
    h.release();
    h.press();
    h.press();
    settle().await;
    assert!(
        h.log.snapshot().is_empty(),
        "capturing a new binding must never start a recording"
    );

    // HTK-049: the settings window polls and the backend has nothing to give.
    // `None` means "nothing usable pressed yet", never "clear the binding".
    assert!(h.hotkeys.end_capture().is_none());
    settle().await;
    assert_eq!(h.settings.read().hotkeys, bound);
    assert!(
        h.log.snapshot().is_empty(),
        "an empty capture result must not be written back as a binding"
    );

    // The suppression is a gate, not a latch: the very next press records.
    h.pipeline.set_capturing_hotkey(false);
    h.press();
    wait_for("recording to start", || h.log.contains(&Ev::CaptureStart)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changing_settings_rearms_the_microphone() {
    let h = Builder::default().build();
    h.settings.write().keep_microphone_active = true;
    h.pipeline.settings_changed();

    wait_for("the microphone to be re-armed", || {
        h.log.contains(&Ev::CapturePrewarm)
    })
    .await;
}

// ---------------------------------------------------------------------------
// Music
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn music_is_paused_for_a_dictation_and_resumed_afterwards() {
    let settings = Settings {
        mute_music: true,
        ..Default::default()
    };
    let h = Builder {
        settings,
        ..Builder::default()
    }
    .build();
    h.hold_and_release().await;

    wait_for("music to resume", || h.log.contains(&Ev::MusicResume)).await;
    let events = h.log.snapshot();
    let paused = events
        .iter()
        .position(|e| *e == Ev::MusicPause)
        .expect("music was paused");
    let resumed = events
        .iter()
        .position(|e| *e == Ev::MusicResume)
        .expect("music was resumed");
    assert!(paused < resumed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn music_is_left_alone_when_the_setting_is_off() {
    let h = Builder::default().build();
    h.hold_and_release().await;
    wait_for("the overlay to hide", || {
        h.log.contains(&Ev::Overlay(OverlayState::Hidden))
    })
    .await;
    assert!(!h.log.contains(&Ev::MusicPause));
}

// ---------------------------------------------------------------------------
// Elapsed readout
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_elapsed_readout_ticks_while_recording_and_stops_at_the_end() {
    let h = Builder::default().build();
    h.press();
    wait_for("a few ticks", || h.log.elapsed.lock().len() >= 4).await;

    // Under 30 s the pill stays minimal: a label would be noise.
    assert!(h
        .log
        .elapsed
        .lock()
        .iter()
        .all(|e| e.label.is_none() && e.warning == 0));

    h.release();
    wait_for("the recording to stop", || {
        h.log.contains(&Ev::CaptureStop(10))
    })
    .await;
    let after_stop = h.log.elapsed.lock().len();
    settle().await;
    assert_eq!(
        h.log.elapsed.lock().len(),
        after_stop,
        "the tick must be cancelled with the recording"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_overlay_action_wire_names_map_to_the_right_action() {
    assert_eq!(OverlayAction::parse("retry"), Some(OverlayAction::Retry));
    assert_eq!(OverlayAction::parse("save"), Some(OverlayAction::Save));
    assert_eq!(
        OverlayAction::parse("dismiss"),
        Some(OverlayAction::Discard)
    );
    assert_eq!(OverlayAction::parse("nonsense"), None);
}
