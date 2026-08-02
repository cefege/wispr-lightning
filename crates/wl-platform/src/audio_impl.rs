//! Microphone capture on top of cpal, identical on macOS and Windows.
//!
//! Three threads meet here and the split between them is the whole design:
//!
//! * the **cpal data callback** is a realtime thread. It pushes `f32` into an
//!   SPSC ring and returns. No locks, no allocation, no FFT.
//! * the **worker** owns the [`Converter`] and does every expensive thing:
//!   downmix, resample, quantize, packetize.
//! * **caller threads** drive `start`/`stop` and never block on the ring.
//!
//! The **cpal error callback** is a fourth, and the most dangerous: it fires on
//! a CoreAudio listener thread or a COM notification thread, and dropping the
//! `Stream` from inside it deadlocks. It therefore does exactly two things —
//! set an atomic and send down a channel — and the teardown happens later on a
//! caller thread.
//!
//! **DV2**: the stream is bound to the chosen device. The Swift original
//! achieved device selection by rewriting the machine-wide default input
//! (`kAudioHardwarePropertyDefaultInputDevice`), which changed the microphone
//! for every other application on the Mac and has no Windows analogue.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    Device, DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedStreamConfig,
};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender, TryRecvError};
use parking_lot::Mutex;
use rtrb::{Consumer, Producer, RingBuffer};
use wl_core::consts::SAMPLE_RATE;

use crate::audio::{AudioCapture, CaptureFault, InputDevice, LevelSink, PacketSink, StartOutcome};
use crate::resample::Converter;
// The two shipping targets each register their own system-wide device
// listeners; the crate compiles elsewhere, just without them.
#[cfg(target_os = "macos")]
use crate::macos::devices::{watch as watch_system_devices, DeviceWatcher};
#[cfg(target_os = "windows")]
use crate::windows::devices::{watch as watch_system_devices, DeviceWatcher};
use crate::{PlatformError, Result};

/// How much audio the ring absorbs before the worker has to have run. Four
/// seconds is far more than the worst scheduler hiccup and costs under a
/// megabyte at 48 kHz stereo.
const RING_SECONDS: usize = 4;

/// Worker idle poll. A packet is 40 ms, so this keeps latency well inside one
/// packet without needing a wakeup from the realtime callback.
const WORKER_POLL: Duration = Duration::from_millis(5);

/// Frames the worker lifts out of the ring per pass.
const DRAIN_CHUNK: usize = 4096;

/// How long `stop` waits for the worker to finish the tail before giving up and
/// returning whatever it already produced. Twenty poll intervals.
const FLUSH_TIMEOUT: Duration = Duration::from_millis(100);

/// Named in the error the UI shows, so a blocked microphone points at the right
/// settings pane instead of reading as a hardware fault.
#[cfg(target_os = "windows")]
const MIC_PERMISSION_HINT: &str = "microphone — Settings > Privacy & security > Microphone > \
     \"Let desktop apps access your microphone\" (ms-settings:privacy-microphone)";
#[cfg(not(target_os = "windows"))]
const MIC_PERMISSION_HINT: &str = "microphone — System Settings > Privacy & Security > Microphone";

/// The capture implementation for this build, ready for `Arc<dyn _>` storage.
///
/// Starts unbound, following the system default; call
/// [`AudioCapture::set_device`] with the persisted `micDeviceId` afterwards.
pub fn capture() -> Result<Arc<dyn AudioCapture>> {
    Ok(Arc::new(CpalCapture::new(None)))
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// Classify a fault reported on a live stream.
///
/// Written against the **Windows** contract, which is the stricter of the two:
/// WASAPI never rebinds an `IAudioClient`, so anything short of an explicit
/// "rerouted" notification means the stream is finished and the supervisor must
/// rebuild it. macOS reroutes transparently and says so with `DeviceChanged`,
/// so the same mapping is also correct there.
///
/// `None` means "not worth telling anyone": the stream is still running.
pub fn map_fault(kind: ErrorKind) -> Option<CaptureFault> {
    match kind {
        // macOS `kAudioDeviceProcessorOverload`, WASAPI glitch reporting.
        ErrorKind::Xrun => Some(CaptureFault::Overrun),
        // macOS only: CoreAudio moved a default-bound stream to a new device
        // and kept it running.
        ErrorKind::DeviceChanged => Some(CaptureFault::DefaultChanged),
        // macOS `kAudioDevicePropertyNominalSampleRate` changed, or Windows
        // `AUDCLNT_E_RESOURCES_INVALIDATED`. Either way the resampler ratio the
        // converter was built with is now a lie.
        ErrorKind::StreamInvalidated => Some(CaptureFault::StreamInvalidated),
        // Scheduling was refused; audio still flows, just with less headroom.
        ErrorKind::RealtimeDenied => None,
        // Everything else — `DeviceNotAvailable` (macOS device-is-alive went
        // false, Windows `AUDCLNT_E_DEVICE_INVALIDATED`), a permission
        // revocation, an exhausted resource, an unclassified backend error, or
        // a variant added in a future cpal — leaves us without a working
        // stream. `ErrorKind` is `#[non_exhaustive]`; treating the unknown as
        // fatal is the safe default, because the alternative is a recording
        // that silently produces nothing.
        _ => Some(CaptureFault::DeviceLost),
    }
}

/// Whether a fault means the current `Stream` object is dead and a new one must
/// be built. Everything else — a default move, a device-list change, an
/// overrun, a silent take — leaves the stream usable.
fn is_terminal(fault: &CaptureFault) -> bool {
    matches!(
        fault,
        CaptureFault::DeviceLost | CaptureFault::StreamInvalidated
    )
}

// ---------------------------------------------------------------------------
// System-wide device-change notifications
// ---------------------------------------------------------------------------

/// Which machine-wide audio property fired.
///
/// The Swift original registers two *distinct* listeners on
/// `kAudioObjectSystemObject` and reacts identically to both. Keeping them
/// apart costs nothing here and lets the supervisor tell "the microphone I am
/// recording on just moved" from "somebody plugged in a webcam" — the first
/// invalidates a running stream, the second does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceChange {
    /// The set of audio devices changed. `kAudioHardwarePropertyDevices`;
    /// `IMMNotificationClient::OnDeviceAdded` / `OnDeviceRemoved` /
    /// `OnDeviceStateChanged`.
    List,
    /// The machine default input moved.
    /// `kAudioHardwarePropertyDefaultInputDevice`;
    /// `IMMNotificationClient::OnDefaultDeviceChanged` for `eCapture` +
    /// `eConsole`.
    DefaultInput,
}

/// How a system-wide device change is reported to the supervisor.
pub fn device_change_fault(change: DeviceChange) -> CaptureFault {
    match change {
        DeviceChange::List => CaptureFault::DevicesChanged,
        DeviceChange::DefaultInput => CaptureFault::DefaultChanged,
    }
}

/// Handle one notification. Runs on a CoreAudio HAL thread or a WASAPI
/// notification thread, so it does exactly two things and returns: mark the
/// resolved device stale, and post the fault. Re-resolving the device or
/// touching the stream from here would re-enter the API that called us.
fn on_device_change(shared: &Shared, faults: &Sender<CaptureFault>, change: DeviceChange) {
    shared.devices_stale.store(true, Ordering::Release);
    let _ = faults.send(device_change_fault(change));
}

/// Start the system-wide listeners for this target.
///
/// Failure is not fatal and does not fail construction: capture still works,
/// the picker just stops live-updating. Say so and carry on.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn watch_devices(shared: &Arc<Shared>, faults: &Sender<CaptureFault>) -> Option<DeviceWatcher> {
    let shared = Arc::clone(shared);
    let faults = faults.clone();
    match watch_system_devices(move |change| on_device_change(&shared, &faults, change)) {
        Ok(watcher) => Some(watcher),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "no system audio device notifications; the microphone picker \
                 will not live-update and a hot-plug will not re-arm the mic"
            );
            None
        }
    }
}

/// Classify a failure to open a device or build a stream.
pub fn map_open_error(err: &cpal::Error) -> PlatformError {
    match err.kind() {
        ErrorKind::PermissionDenied => PlatformError::PermissionDenied(MIC_PERMISSION_HINT),
        // cpal's WASAPI backend maps the `AUDCLNT_E_*` codes explicitly and
        // lets a bare `E_ACCESSDENIED` fall through to `BackendError`. A
        // desktop app blocked by the global microphone toggle gets exactly
        // that, and Win32 has no API to ask, so this is the best signal
        // available — and it is right far more often than it is wrong.
        #[cfg(target_os = "windows")]
        ErrorKind::BackendError => PlatformError::PermissionDenied(MIC_PERMISSION_HINT),
        ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => PlatformError::NoInputDevice,
        _ => PlatformError::AudioDevice(err.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct Shared {
    /// Gates the realtime callback. False during pre-warm, which is how the
    /// stream stays open while its samples go nowhere.
    armed: AtomicBool,
    /// Samples the callback could not fit into the ring.
    overruns: AtomicUsize,
    /// Set by the error callback when the stream cannot be recovered.
    dead: AtomicBool,
    /// Set by a system-wide device notification. The open session resolved a
    /// persisted id to a particular device once; a change to the device set or
    /// to the machine default means that answer is now a guess, so the next
    /// `prewarm`/`start` re-resolves instead of trusting it. This is the
    /// Swift original's `invalidateDeviceCache()`.
    devices_stale: AtomicBool,
    packets: Mutex<Vec<Vec<i16>>>,
    /// Where complete PCM packets go for live transcription.
    packet_sink: Mutex<Option<PacketSink>>,
    /// Where the per-packet level goes, when anything is listening.
    ///
    /// Separate from `packets` so the worker never holds the packet lock
    /// across a call into UI code, and so installing or clearing a sink from a
    /// caller thread cannot contend with the transcript accumulating.
    level_sink: Mutex<Option<LevelSink>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            overruns: AtomicUsize::new(0),
            dead: AtomicBool::new(false),
            devices_stale: AtomicBool::new(false),
            packets: Mutex::new(Vec::new()),
            packet_sink: Mutex::new(None),
            level_sink: Mutex::new(None),
        }
    }
}

enum Cmd {
    /// Drain the ring, flush the converter's tail, then acknowledge.
    Flush(Sender<()>),
    Shutdown,
}

// ---------------------------------------------------------------------------
// Session: one open stream plus its worker
// ---------------------------------------------------------------------------

struct Session {
    stream: Stream,
    cmd: Sender<Cmd>,
    worker: Option<JoinHandle<()>>,
    /// The device id this session was built for, so a later `set_device` can be
    /// detected as making it stale.
    requested: Option<String>,
    /// The device the stream is actually bound to, which is a different
    /// question from `requested`: it is the fallback device after a miss, and
    /// the resolved default when nothing was requested. Nothing else can answer
    /// "which microphone is this stream really on".
    bound: InputDevice,
    /// The configured device was gone and the system default is in use.
    fell_back: bool,
}

impl Drop for Session {
    fn drop(&mut self) {
        // Order matters: stop the producer before the consumer goes away.
        // `pause` is best-effort — a device that vanished will refuse — and the
        // stream is dropped immediately after this returns regardless.
        let _ = self.stream.pause();
        let _ = self.cmd.send(Cmd::Shutdown);
        if let Some(handle) = self.worker.take() {
            if handle.join().is_err() {
                tracing::error!("audio worker thread panicked");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The capture implementation
// ---------------------------------------------------------------------------

pub struct CpalCapture {
    inner: Mutex<Inner>,
    shared: Arc<Shared>,
    fault_tx: Sender<CaptureFault>,
    fault_rx: Receiver<CaptureFault>,
    /// The machine-wide device listeners (MATRIX AUD-019). Held for its
    /// `Drop`, which is what deregisters them; `None` when the platform
    /// refused to install them, which costs live picker updates and nothing
    /// else.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    _devices: Option<DeviceWatcher>,
}

struct Inner {
    session: Option<Session>,
    /// `None` follows the system default input.
    requested: Option<String>,
}

/// Whether an open session built for `built_for` can still serve a recording
/// of `requested`.
///
/// Split out of [`CpalCapture::ensure_session`] so every reason a session goes
/// stale can be pinned by a test without opening a microphone.
fn session_is_stale(
    built_for: Option<&str>,
    requested: Option<&str>,
    dead: bool,
    devices_stale: bool,
) -> bool {
    built_for != requested || dead || devices_stale
}

impl CpalCapture {
    /// `device` is a persisted [`cpal::DeviceId`] string (the CoreAudio UID or
    /// the WASAPI endpoint id, prefixed with its host), or `None` to follow the
    /// system default.
    ///
    /// Constructing does not touch the microphone. It does register the
    /// system-wide device listeners, which is a HAL/MMDevice subscription: no
    /// TCC prompt, no recording indicator, no open endpoint.
    pub fn new(device: Option<&str>) -> Self {
        let (fault_tx, fault_rx) = unbounded();
        let shared = Arc::new(Shared::new());
        Self {
            inner: Mutex::new(Inner {
                session: None,
                requested: device.map(str::to_owned),
            }),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            _devices: watch_devices(&shared, &fault_tx),
            shared,
            fault_tx,
            fault_rx,
        }
    }

    /// Open a stream if there is not already a usable one.
    fn ensure_session(&self, inner: &mut Inner) -> Result<()> {
        let stale = match inner.session.as_ref() {
            None => true,
            Some(s) => session_is_stale(
                s.requested.as_deref(),
                inner.requested.as_deref(),
                self.shared.dead.load(Ordering::Acquire),
                self.shared.devices_stale.load(Ordering::Acquire),
            ),
        };
        if !stale {
            return Ok(());
        }
        // Drop first, so the old device is released before the new one is
        // opened; some drivers refuse a second client on the same endpoint.
        inner.session = None;
        self.shared.dead.store(false, Ordering::Release);
        self.shared.devices_stale.store(false, Ordering::Release);
        inner.session = Some(open_session(
            inner.requested.as_deref(),
            &self.shared,
            &self.fault_tx,
        )?);
        Ok(())
    }

    /// The device the open stream is bound to, or `None` when no stream is
    /// open.
    ///
    /// [`AudioCapture::set_device`] records a *request*; this reports what that
    /// request resolved to once a stream exists, which is the only way to tell
    /// a stream bound to the chosen microphone from one that silently fell back
    /// to the system default. `is_default` is recomputed on each call so the
    /// answer stays true after the machine default moves underneath us.
    pub fn bound_device(&self) -> Option<InputDevice> {
        let inner = self.inner.lock();
        let bound = &inner.session.as_ref()?.bound;
        let is_default = cpal::default_host()
            .default_input_device()
            .and_then(|d| d.id().ok())
            .is_some_and(|id| id.to_string() == bound.id);
        Some(InputDevice {
            is_default,
            ..bound.clone()
        })
    }
}

impl Default for CpalCapture {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AudioCapture for CpalCapture {
    fn list_devices(&self) -> Result<Vec<InputDevice>> {
        let host = cpal::default_host();
        let default = host
            .default_input_device()
            .and_then(|d| d.id().ok())
            .map(|id| id.to_string());

        let devices = host
            .input_devices()
            .map_err(|e| PlatformError::AudioDevice(format!("cannot enumerate inputs: {e}")))?;

        let mut out = Vec::new();
        for device in devices {
            // `describe` returns `None` when the device was unplugged between
            // enumeration and now. Skip it rather than failing the whole list —
            // the user is looking at a picker.
            let Some(mut info) = describe(&device) else {
                continue;
            };
            info.is_default = default.as_deref() == Some(info.id.as_str());
            out.push(info);
        }
        Ok(out)
    }

    /// Open the device now so the first dictation does not pay the 100–300 ms
    /// open cost (much worse over Bluetooth).
    ///
    /// On macOS this lights the orange microphone indicator and lists the app
    /// under "microphone in use" in Control Center for as long as the stream is
    /// held. That reads as "always listening" for a menu-bar app, which is why
    /// `keepMicrophoneActive` defaults to off.
    fn prewarm(&self) -> Result<()> {
        let mut inner = self.inner.lock();
        self.ensure_session(&mut inner)
    }

    fn release(&self) -> Result<()> {
        // Lock before checking: `start` holds this lock while arming, so a
        // concurrent start cannot slip in between the check and the teardown
        // and lose the stream it just built.
        let mut inner = self.inner.lock();
        if self.is_recording() {
            // Never yank the microphone out from under a live dictation.
            return Ok(());
        }
        // Dropping the session pauses the stream, shuts the worker down and
        // joins it. This is the whole point of the call: on macOS it is what
        // turns the orange microphone indicator off, and `actor.rs` relies on
        // it to discard a stream a device fault has invalidated before the
        // next recording rebuilds one.
        inner.session = None;
        Ok(())
    }

    fn start(&self) -> Result<StartOutcome> {
        let mut inner = self.inner.lock();
        self.ensure_session(&mut inner)?;

        self.shared.packets.lock().clear();
        self.shared.overruns.store(0, Ordering::Relaxed);

        let fell_back = inner.session.as_ref().is_some_and(|s| s.fell_back);
        let requested = inner.requested.clone();
        self.shared.armed.store(true, Ordering::Release);

        Ok(match (fell_back, requested) {
            (true, Some(requested)) => StartOutcome::StartedWithFallback { requested },
            _ => StartOutcome::Started,
        })
    }

    /// Stop accumulating and return the recording.
    ///
    /// The stream stays open afterwards. The Swift original did the same, for a
    /// concrete reason: tearing the engine down and back up makes CoreAudio
    /// reconfigure the device, which drops Bluetooth audio for a beat.
    fn stop(&self) -> Vec<Vec<i16>> {
        self.shared.armed.store(false, Ordering::Release);

        let inner = self.inner.lock();
        if let Some(session) = inner.session.as_ref() {
            let (ack_tx, ack_rx) = bounded(1);
            if session.cmd.send(Cmd::Flush(ack_tx)).is_ok()
                && ack_rx.recv_timeout(FLUSH_TIMEOUT).is_err()
            {
                tracing::warn!(
                    "audio worker did not finish within {FLUSH_TIMEOUT:?}; \
                     returning the packets it had produced"
                );
            }
        }
        drop(inner);

        std::mem::take(&mut *self.shared.packets.lock())
    }

    fn is_recording(&self) -> bool {
        self.shared.armed.load(Ordering::Acquire)
    }

    fn take_faults(&self) -> Vec<CaptureFault> {
        let mut faults: Vec<_> = self.fault_rx.try_iter().collect();
        if self.shared.overruns.swap(0, Ordering::Relaxed) > 0 {
            // Collapsed to one report: a starved worker would otherwise emit
            // thousands, and the supervisor's reaction is the same either way.
            faults.push(CaptureFault::Overrun);
        }
        faults
    }

    /// Record the device to bind to. The change takes effect when the next
    /// stream is built, so switching microphones mid-dictation does not
    /// interrupt the recording in progress.
    fn set_device(&self, id: Option<&str>) -> Result<()> {
        self.inner.lock().requested = id.map(str::to_owned);
        Ok(())
    }

    fn set_level_sink(&self, sink: Option<LevelSink>) {
        *self.shared.level_sink.lock() = sink;
    }

    fn set_packet_sink(&self, sink: Option<PacketSink>) {
        *self.shared.packet_sink.lock() = sink;
    }
}

// ---------------------------------------------------------------------------
// Opening a stream
// ---------------------------------------------------------------------------

/// Identify a device the way the picker does: the persisted id plus a display
/// name, falling back to the raw id when the description is unreadable.
///
/// `None` means the device answered neither — it was unplugged mid-query.
/// `is_default` is left false; only the caller knows the current default.
fn describe(device: &Device) -> Option<InputDevice> {
    let id = device.id().ok()?;
    let name = device
        .description()
        .map(|d| d.name().to_owned())
        .unwrap_or_else(|_| id.id().to_owned());
    Some(InputDevice {
        id: id.to_string(),
        name,
        is_default: false,
    })
}

/// Resolve the persisted id to a live device. Returns the device and whether we
/// had to fall back to the system default.
fn resolve_device(requested: Option<&str>) -> Result<(Device, bool)> {
    let host = cpal::default_host();
    let default = || {
        host.default_input_device()
            .ok_or(PlatformError::NoInputDevice)
    };

    let Some(requested) = requested else {
        return Ok((default()?, false));
    };

    // A `DeviceId` string carries its host prefix, so a config written on macOS
    // cannot accidentally resolve on Windows — it fails to parse here.
    let found = requested
        .parse::<DeviceId>()
        .ok()
        .and_then(|id| host.device_by_id(&id));

    match found {
        Some(device) => Ok((device, false)),
        None => {
            tracing::warn!(%requested, "configured microphone is unavailable, using system default");
            Ok((default()?, true))
        }
    }
}

/// Choose a stream format, preferring one that removes the resampler.
///
/// A device that natively offers 16 kHz mono lets the converter run as a plain
/// quantizer. Do not count on it: macOS built-in microphones typically only
/// publish 48 kHz.
fn pick_config(device: &Device) -> Result<SupportedStreamConfig> {
    if let Ok(ranges) = device.supported_input_configs() {
        let native = ranges.into_iter().find(|r| {
            r.sample_format() == SampleFormat::F32
                && r.channels() == 1
                && (r.min_sample_rate()..=r.max_sample_rate()).contains(&SAMPLE_RATE)
        });
        if let Some(range) = native {
            return Ok(range.with_sample_rate(SAMPLE_RATE));
        }
    }
    device
        .default_input_config()
        .map_err(|e| map_open_error(&e))
}

fn open_session(
    requested: Option<&str>,
    shared: &Arc<Shared>,
    faults: &Sender<CaptureFault>,
) -> Result<Session> {
    let (device, fell_back) = resolve_device(requested)?;
    let bound = describe(&device).unwrap_or_else(|| InputDevice {
        id: String::new(),
        name: "<unreadable>".to_owned(),
        is_default: false,
    });
    let supported = pick_config(&device)?;
    let sample_format = supported.sample_format();
    let config = supported.config();

    let converter = Converter::new(config.sample_rate, config.channels)?;

    let capacity = config.sample_rate as usize * usize::from(config.channels) * RING_SECONDS;
    let (producer, consumer) = RingBuffer::<f32>::new(capacity);

    let stream = build_stream(
        &device,
        config,
        sample_format,
        producer,
        Arc::clone(shared),
        faults.clone(),
    )
    .map_err(|e| map_open_error(&e))?;

    let (cmd_tx, cmd_rx) = unbounded();
    let worker_shared = Arc::clone(shared);
    let worker_faults = faults.clone();
    let worker = std::thread::Builder::new()
        .name("wl-audio-worker".into())
        .spawn(move || run_worker(consumer, converter, worker_shared, worker_faults, cmd_rx))
        .map_err(PlatformError::Io)?;

    let session = Session {
        stream,
        cmd: cmd_tx,
        worker: Some(worker),
        requested: requested.map(str::to_owned),
        bound,
        fell_back,
    };

    // Built streams start paused. If this fails, dropping `session` shuts the
    // worker down for us.
    session.stream.play().map_err(|e| map_open_error(&e))?;

    tracing::info!(
        device = %session.bound.id,
        name = %session.bound.name,
        rate = config.sample_rate,
        channels = config.channels,
        ?sample_format,
        fell_back,
        "microphone stream open"
    );
    Ok(session)
}

fn build_stream(
    device: &Device,
    config: StreamConfig,
    format: SampleFormat,
    producer: Producer<f32>,
    shared: Arc<Shared>,
    faults: Sender<CaptureFault>,
) -> std::result::Result<Stream, cpal::Error> {
    match format {
        SampleFormat::F32 => stream_of::<f32>(device, config, producer, shared, faults),
        SampleFormat::I16 => stream_of::<i16>(device, config, producer, shared, faults),
        SampleFormat::U16 => stream_of::<u16>(device, config, producer, shared, faults),
        SampleFormat::I32 => stream_of::<i32>(device, config, producer, shared, faults),
        SampleFormat::I8 => stream_of::<i8>(device, config, producer, shared, faults),
        SampleFormat::U8 => stream_of::<u8>(device, config, producer, shared, faults),
        SampleFormat::F64 => stream_of::<f64>(device, config, producer, shared, faults),
        other => Err(cpal::Error::with_message(
            ErrorKind::UnsupportedConfig,
            format!("microphone sample format {other} is not supported"),
        )),
    }
}

fn stream_of<T>(
    device: &Device,
    config: StreamConfig,
    mut producer: Producer<f32>,
    shared: Arc<Shared>,
    faults: Sender<CaptureFault>,
) -> std::result::Result<Stream, cpal::Error>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let capture = Arc::clone(&shared);
    device.build_input_stream::<T, _, _>(
        config,
        move |data, _| {
            // Realtime thread. Everything here is wait-free: two atomics and a
            // memcpy-shaped loop into the ring.
            if !capture.armed.load(Ordering::Acquire) {
                return;
            }
            let room = producer.slots().min(data.len());
            if room < data.len() {
                capture
                    .overruns
                    .fetch_add(data.len() - room, Ordering::Relaxed);
            }
            if room == 0 {
                return;
            }
            match producer.write_chunk_uninit(room) {
                Ok(chunk) => {
                    chunk.fill_from_iter(data[..room].iter().map(|&s| f32::from_sample_(s)));
                }
                Err(_) => {
                    // The consumer went away: the session is being torn down.
                    capture.overruns.fetch_add(data.len(), Ordering::Relaxed);
                }
            }
        },
        move |err| {
            // CoreAudio listener thread or COM notification thread. Dropping
            // the `Stream` from here deadlocks, so record and get out.
            let Some(fault) = map_fault(err.kind()) else {
                tracing::debug!(%err, "capture stream reported a non-fatal condition");
                return;
            };
            tracing::warn!(%err, ?fault, "capture stream fault");
            if is_terminal(&fault) {
                shared.dead.store(true, Ordering::Release);
            }
            let _ = faults.send(fault);
        },
        None,
    )
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

fn run_worker(
    mut ring: Consumer<f32>,
    mut converter: Converter,
    shared: Arc<Shared>,
    faults: Sender<CaptureFault>,
    cmds: Receiver<Cmd>,
) {
    let mut scratch = vec![0.0f32; DRAIN_CHUNK];
    loop {
        let moved = pump(&mut ring, &mut scratch, &mut converter, &shared);

        match cmds.try_recv() {
            Ok(Cmd::Shutdown) | Err(TryRecvError::Disconnected) => return,
            Ok(Cmd::Flush(ack)) => {
                // One more sweep: the callback may have pushed between the last
                // pump and the arm flag clearing, and that audio is the end of
                // the user's sentence.
                while pump(&mut ring, &mut scratch, &mut converter, &shared) > 0 {}

                let mut tail = Vec::new();
                if let Err(e) = converter.finish(|packet| tail.push(packet.to_vec())) {
                    tracing::error!(%e, "flushing the audio tail failed");
                }
                deliver(&shared, tail);
                if converter.input_was_silent() {
                    let _ = faults.send(CaptureFault::SilentInput);
                }
                converter.reset();
                let _ = ack.send(());
            }
            Err(TryRecvError::Empty) => {}
        }

        if moved == 0 {
            std::thread::sleep(WORKER_POLL);
        }
    }
}

/// Move one batch out of the ring and through the converter. Returns the number
/// of samples consumed.
fn pump(
    ring: &mut Consumer<f32>,
    scratch: &mut [f32],
    converter: &mut Converter,
    shared: &Shared,
) -> usize {
    let taken = {
        let (filled, _) = ring.pop_partial_slice(scratch);
        filled.len()
    };
    if taken == 0 {
        return 0;
    }

    let mut produced = Vec::new();
    let outcome =
        converter.push_interleaved(&scratch[..taken], |packet| produced.push(packet.to_vec()));
    if let Err(e) = outcome {
        tracing::error!(%e, "audio conversion failed; dropped {taken} samples");
    }
    deliver(shared, produced);
    taken
}

/// Bank converted packets, feed the live transcription stream, and publish the
/// level of the newest packet.
///
/// The single place packets become visible, so the meter and live stream cannot
/// drift out of step with the retry buffer. Each packet is 640 samples at
/// 16 kHz — 40 ms. Sinks are cloned out from under their locks before calling
/// user code, so installing or clearing one cannot deadlock the audio worker.
fn deliver(shared: &Shared, packets: Vec<Vec<i16>>) {
    let Some(newest) = packets.last() else {
        return;
    };
    let level = normalized_level(newest);

    let packet_sink = shared.packet_sink.lock().clone();
    if let Some(sink) = packet_sink {
        for packet in &packets {
            sink(packet);
        }
    }
    shared.packets.lock().extend(packets);

    let level_sink = shared.level_sink.lock().clone();
    if let Some(sink) = level_sink {
        sink(level);
    }
}

/// Normalized 0.0–1.0 loudness of one packet, for the recording meter.
///
/// RMS to dBFS, clamped to the bottom 60 dB and rescaled: −60 dBFS and quieter
/// reads 0.0, full scale reads 1.0, and the curve is linear in decibels in
/// between. Linear amplitude would be useless here — speech at a comfortable
/// level sits around 0.05 of full scale, so a linear meter is a flat line.
///
/// Packets are already downmixed to mono, so this is the single channel the
/// transcription backend will hear, not an average over channels the user
/// cannot control.
fn normalized_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    // f64 accumulation: a 640-sample packet of full-scale audio sums to about
    // 6.7e11, which f32 cannot hold without losing the quiet samples entirely.
    let sum_squares: f64 = samples
        .iter()
        .map(|&s| {
            let normalized = f64::from(s) / f64::from(i16::MAX);
            normalized * normalized
        })
        .sum();
    let rms = (sum_squares / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        // log10(0) is −inf. Digital silence is the floor by definition.
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db.clamp(-60.0, 0.0) + 60.0) / 60.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `ErrorKind` cpal 0.18 defines, so a new variant appearing in a
    /// dependency bump is a visible decision rather than a surprise.
    const ALL_KINDS: &[ErrorKind] = &[
        ErrorKind::DeviceBusy,
        ErrorKind::DeviceChanged,
        ErrorKind::DeviceNotAvailable,
        ErrorKind::HostUnavailable,
        ErrorKind::InvalidInput,
        ErrorKind::PermissionDenied,
        ErrorKind::RealtimeDenied,
        ErrorKind::ResourceExhausted,
        ErrorKind::StreamInvalidated,
        ErrorKind::UnsupportedConfig,
        ErrorKind::UnsupportedOperation,
        ErrorKind::Xrun,
        ErrorKind::BackendError,
        ErrorKind::Other,
    ];

    #[test]
    fn the_faults_the_supervisor_acts_on_map_from_the_right_cpal_kinds() {
        // macOS device-is-alive false / Windows AUDCLNT_E_DEVICE_INVALIDATED.
        assert_eq!(
            map_fault(ErrorKind::DeviceNotAvailable),
            Some(CaptureFault::DeviceLost)
        );
        // macOS nominal-sample-rate change / Windows
        // AUDCLNT_E_RESOURCES_INVALIDATED.
        assert_eq!(
            map_fault(ErrorKind::StreamInvalidated),
            Some(CaptureFault::StreamInvalidated)
        );
        // macOS only; Windows never rebinds and so never reports this.
        assert_eq!(
            map_fault(ErrorKind::DeviceChanged),
            Some(CaptureFault::DefaultChanged)
        );
        assert_eq!(map_fault(ErrorKind::Xrun), Some(CaptureFault::Overrun));
    }

    #[test]
    fn every_other_cpal_error_kind_is_treated_as_a_dead_stream() {
        let handled = [
            ErrorKind::DeviceNotAvailable,
            ErrorKind::StreamInvalidated,
            ErrorKind::DeviceChanged,
            ErrorKind::Xrun,
            ErrorKind::RealtimeDenied,
        ];
        for &kind in ALL_KINDS {
            if handled.contains(&kind) {
                continue;
            }
            assert_eq!(
                map_fault(kind),
                Some(CaptureFault::DeviceLost),
                "{kind:?} must not be silently ignored: a stream that stopped \
                 delivering audio without telling anyone is a lost recording"
            );
        }
    }

    #[test]
    fn a_refused_realtime_promotion_is_not_reported_as_a_fault() {
        // Audio still flows; surfacing this would make the UI cry wolf.
        assert_eq!(map_fault(ErrorKind::RealtimeDenied), None);
    }

    #[test]
    fn only_a_dead_stream_forces_a_rebuild() {
        assert!(is_terminal(&CaptureFault::DeviceLost));
        assert!(is_terminal(&CaptureFault::StreamInvalidated));
        assert!(
            !is_terminal(&CaptureFault::DefaultChanged),
            "macOS rerouted the stream for us; rebuilding would drop audio"
        );
        assert!(!is_terminal(&CaptureFault::Overrun));
        assert!(!is_terminal(&CaptureFault::SilentInput));
        assert!(
            !is_terminal(&CaptureFault::DevicesChanged),
            "a microphone appearing elsewhere on the machine says nothing \
             about the stream we are holding"
        );
    }

    // -- System-wide device changes (MATRIX AUD-019) ----------------------

    /// The two HAL properties are two different facts and must not collapse
    /// onto one fault: a default move invalidates a default-bound stream, a
    /// list change does not.
    #[test]
    fn the_two_system_properties_report_different_faults() {
        assert_eq!(
            device_change_fault(DeviceChange::DefaultInput),
            CaptureFault::DefaultChanged
        );
        assert_eq!(
            device_change_fault(DeviceChange::List),
            CaptureFault::DevicesChanged
        );
    }

    /// Both notifications reach the supervisor *and* invalidate the resolved
    /// device, which is `invalidateDeviceCache()` in the Swift original.
    #[test]
    fn a_device_notification_posts_a_fault_and_invalidates_the_resolution() {
        for (change, expected) in [
            (DeviceChange::List, CaptureFault::DevicesChanged),
            (DeviceChange::DefaultInput, CaptureFault::DefaultChanged),
        ] {
            let shared = Shared::new();
            let (tx, rx) = unbounded();
            assert!(!shared.devices_stale.load(Ordering::Acquire));

            on_device_change(&shared, &tx, change);

            assert!(
                shared.devices_stale.load(Ordering::Acquire),
                "{change:?} must invalidate the cached device resolution"
            );
            assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![expected]);
        }
    }

    /// A burst of notifications — one unplug produces several — must not
    /// queue up work beyond the one flag and one fault each.
    #[test]
    fn repeated_notifications_stay_idempotent_on_the_flag() {
        let shared = Shared::new();
        let (tx, rx) = unbounded();
        for _ in 0..5 {
            on_device_change(&shared, &tx, DeviceChange::List);
        }
        assert!(shared.devices_stale.load(Ordering::Acquire));
        assert_eq!(rx.try_iter().count(), 5, "the supervisor coalesces, not us");
    }

    /// The invalidation has teeth: a session that would otherwise be reused
    /// is thrown away so the next recording re-resolves the device.
    #[test]
    fn an_invalidated_resolution_forces_the_next_session_to_re_resolve() {
        let mic = Some("coreaudio:BuiltInMicrophoneDevice");

        assert!(
            !session_is_stale(mic, mic, false, false),
            "nothing changed; re-opening would cost 100-300 ms for nothing"
        );
        assert!(
            session_is_stale(mic, mic, false, true),
            "the device set moved, so the session's resolution is a guess"
        );
        // The pre-existing reasons still stand on their own.
        assert!(session_is_stale(mic, None, false, false));
        assert!(session_is_stale(mic, mic, true, false));
        // Following the system default is not exempt: that is precisely the
        // binding a default-input change invalidates.
        assert!(session_is_stale(None, None, false, true));
        assert!(!session_is_stale(None, None, false, false));
    }

    #[test]
    fn a_denied_microphone_is_reported_as_a_permission_problem_not_a_hardware_one() {
        let err = map_open_error(&cpal::Error::new(ErrorKind::PermissionDenied));
        assert!(
            matches!(err, PlatformError::PermissionDenied(_)),
            "got {err:?}"
        );
        assert!(!err.is_transient(), "retrying a TCC denial cannot succeed");
        assert!(
            err.to_string().to_lowercase().contains("microphone"),
            "the message must name the setting the user has to change: {err}"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_unclassified_backend_errors_point_at_the_privacy_toggle() {
        // cpal maps the AUDCLNT_E_* codes explicitly and drops a bare
        // E_ACCESSDENIED into BackendError, which is what a desktop app gets
        // when "Let desktop apps access your microphone" is off.
        let err = map_open_error(&cpal::Error::new(ErrorKind::BackendError));
        assert!(matches!(err, PlatformError::PermissionDenied(_)));
        assert!(err.to_string().contains("ms-settings:privacy-microphone"));
    }

    #[test]
    fn a_missing_device_is_reported_as_no_input_rather_than_a_generic_failure() {
        // The UI distinguishes these: one says "plug in a microphone", the
        // other says "something went wrong".
        assert!(matches!(
            map_open_error(&cpal::Error::new(ErrorKind::DeviceNotAvailable)),
            PlatformError::NoInputDevice
        ));
        assert!(matches!(
            map_open_error(&cpal::Error::new(ErrorKind::UnsupportedConfig)),
            PlatformError::AudioDevice(_)
        ));
    }

    #[test]
    fn a_fresh_capture_is_idle_and_holds_no_device() {
        // The app builds this at launch, long before the user has granted
        // anything; construction must not touch the microphone.
        let capture = CpalCapture::new(Some("coreaudio:BuiltInMicrophoneDevice"));
        assert!(!capture.is_recording());
        assert!(capture.take_faults().is_empty());
        assert!(capture.inner.lock().session.is_none());
    }

    #[test]
    fn selecting_a_device_records_it_without_disturbing_a_live_stream() {
        // Switching microphones mid-dictation must not interrupt the take; the
        // new device is picked up when the next stream is built.
        let capture = CpalCapture::new(None);
        capture
            .set_device(Some("wasapi:{0.0.1.00000000}.{guid}"))
            .unwrap();
        assert_eq!(
            capture.inner.lock().requested.as_deref(),
            Some("wasapi:{0.0.1.00000000}.{guid}")
        );
        capture.set_device(None).unwrap();
        assert!(capture.inner.lock().requested.is_none());
    }

    #[test]
    fn dropped_buffers_are_reported_once_and_then_cleared() {
        let capture = CpalCapture::new(None);
        capture.shared.overruns.store(1_920, Ordering::Relaxed);
        assert_eq!(capture.take_faults(), vec![CaptureFault::Overrun]);
        assert!(
            capture.take_faults().is_empty(),
            "the same dropped buffers must not be reported forever"
        );
    }

    /// One packet's worth of a constant amplitude, which makes the RMS exactly
    /// that amplitude and the expected level arithmetic rather than empirical.
    fn constant_packet(amplitude: i16) -> Vec<i16> {
        vec![amplitude; wl_core::consts::CHUNK_SAMPLES]
    }

    /// The two anchors the whole meter is calibrated against.
    #[test]
    fn the_level_curve_pins_silence_to_zero_and_full_scale_to_one() {
        assert_eq!(normalized_level(&constant_packet(0)), 0.0);
        assert_eq!(normalized_level(&constant_packet(i16::MAX)), 1.0);
    }

    /// −60 dBFS is the floor, and anything quieter clamps there rather than
    /// reporting a negative level the UI would render as a bar pointing the
    /// wrong way.
    #[test]
    fn the_level_curve_floors_at_minus_sixty_decibels() {
        // The nearest i16 to −60 dBFS is 33/32767, which is −59.94 dB, so the
        // floor is approached rather than hit exactly. A whole 1 % of the
        // meter's range is still 60× the quantization error.
        let floor = normalized_level(&constant_packet(33));
        assert!(
            floor < 0.01,
            "−60 dBFS should sit on the floor, got {floor}"
        );
        assert_eq!(
            normalized_level(&constant_packet(1)),
            0.0,
            "−90 dBFS must clamp, not report a negative level"
        );
    }

    /// Linear in decibels, not in amplitude: halving the amplitude is −6.02 dB,
    /// which is a tenth of the 60 dB range. A linear-amplitude meter would
    /// report 0.5 here, which is the bug this test exists to catch.
    #[test]
    fn the_level_curve_is_linear_in_decibels() {
        let half = normalized_level(&constant_packet(i16::MAX / 2));
        assert!(
            (half - (60.0 - 6.0206) / 60.0).abs() < 1e-3,
            "half amplitude should read ~0.8997, got {half}"
        );

        let tenth = normalized_level(&constant_packet(i16::MAX / 10));
        assert!(
            (tenth - (60.0 - 20.0) / 60.0).abs() < 1e-3,
            "a tenth of full scale is −20 dBFS, so ~0.6667, got {tenth}"
        );
    }

    /// RMS, not peak or mean: a signal that swings symmetrically about zero
    /// averages to nothing, and a mean-based meter would call it silence.
    #[test]
    fn the_level_curve_measures_rms_rather_than_mean() {
        let square: Vec<i16> = (0..wl_core::consts::CHUNK_SAMPLES)
            .map(|i| if i % 2 == 0 { i16::MAX } else { -i16::MAX })
            .collect();
        assert_eq!(normalized_level(&square), 1.0);
    }

    /// The level rises with amplitude across the whole audible range, so the
    /// bars never move backwards as the user speaks up, and never leave 0..=1.
    #[test]
    fn the_level_curve_rises_monotonically_with_amplitude() {
        let mut previous = f32::MIN;
        // Loudest last: each step is 1 dB up from the one before it.
        for attenuation_db in (0..=59).rev() {
            let amplitude =
                (f64::from(i16::MAX) * 10f64.powf(-f64::from(attenuation_db) / 20.0)) as i16;
            let level = normalized_level(&constant_packet(amplitude));
            assert!(
                level > previous,
                "level did not rise at −{attenuation_db} dBFS: {previous} then {level}"
            );
            assert!((0.0..=1.0).contains(&level), "{level} is outside 0..=1");
            previous = level;
        }
    }

    /// An empty slice would divide by zero on its way to a NaN the UI would
    /// render as a blank strip.
    #[test]
    fn an_empty_packet_reads_as_silence() {
        assert_eq!(normalized_level(&[]), 0.0);
    }

    /// The sink is what the overlay hangs off; delivering packets without
    /// firing it, or firing it more than once per pump, both break the meter.
    #[test]
    fn delivering_packets_publishes_the_newest_level_once() {
        let shared = Shared::new();
        let seen: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        *shared.level_sink.lock() = Some(Arc::new(move |level| recorder.lock().push(level)));

        deliver(&shared, vec![constant_packet(i16::MAX), constant_packet(0)]);

        assert_eq!(
            *seen.lock(),
            vec![0.0],
            "the newest packet's level, published exactly once"
        );
        assert_eq!(
            shared.packets.lock().len(),
            2,
            "both packets must still reach the transcript"
        );
    }

    /// Nothing captured means nothing published — and, more importantly, no
    /// spurious zero that would make the meter twitch at idle.
    #[test]
    fn delivering_nothing_does_not_publish_a_level() {
        let shared = Shared::new();
        let fired = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&fired);
        *shared.level_sink.lock() = Some(Arc::new(move |_| flag.store(true, Ordering::Release)));

        deliver(&shared, Vec::new());

        assert!(!fired.load(Ordering::Acquire));
        assert!(shared.packets.lock().is_empty());
    }

    /// Publishing with no sink installed is the normal state whenever the
    /// overlay is hidden, so it must not cost the packets.
    #[test]
    fn packets_are_banked_even_with_no_sink_installed() {
        let shared = Shared::new();
        deliver(&shared, vec![constant_packet(1_000)]);
        assert_eq!(shared.packets.lock().len(), 1);
    }
}
