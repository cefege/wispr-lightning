//! Microphone capture.
//!
//! Produces exactly what the transcription protocol wants: 16 kHz mono
//! signed-16-bit little-endian PCM in 640-sample (1280-byte) packets,
//! regardless of what the hardware natively offers.

use std::sync::Arc;

use crate::Result;

/// An input device as shown in the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    /// Stable identifier that survives restarts: the CoreAudio UID on macOS,
    /// the WASAPI endpoint id on Windows. Persist this, never the name — two
    /// identical USB microphones share a name.
    pub id: String,
    pub name: String,
    /// Whether this is the system's current default input.
    pub is_default: bool,
}

/// Outcome of starting a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    Started,
    /// The configured device was unavailable, so the system default is in use.
    StartedWithFallback {
        requested: String,
    },
}

/// Why capture stopped or degraded, delivered asynchronously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureFault {
    /// The device went away. On Windows this is terminal for the stream even
    /// when a replacement default exists, so the supervisor must rebuild.
    DeviceLost,
    /// The stream is no longer valid, typically a sample-rate change. The
    /// resampler ratio is now wrong; rebuild rather than continue.
    StreamInvalidated,
    /// The default device changed underneath a default-bound stream. macOS
    /// reroutes transparently; Windows does not.
    DefaultChanged,
    /// The set of audio devices changed: something was plugged in, unplugged,
    /// disabled or re-enabled. The picker is stale and any cached resolution
    /// of a persisted device id is now a guess, but the open stream — if
    /// there is one — is still perfectly valid, which is what separates this
    /// from [`CaptureFault::DefaultChanged`]. Rebuilding on it would turn a
    /// user plugging in headphones into an audible gap in their dictation.
    DevicesChanged,
    /// A buffer was dropped. Informational.
    Overrun,
    /// The whole recording was digital silence. A *heuristic*, not a
    /// permission API: on Windows a microphone blocked by the global privacy
    /// toggle often yields all-zero samples instead of an error, and this is
    /// the only signal that distinguishes it from a quiet room. Raised at most
    /// once per recording, and only when *every* sample of a long-enough take
    /// was exactly zero, so a user who pauses before speaking is never scolded.
    SilentInput,
}

/// Receives each complete 40 ms PCM packet while capture is active.
///
/// Invoked from the audio worker, never the realtime device callback. The sink
/// must copy or enqueue the borrowed packet and return immediately; blocking
/// here stalls capture and can cause an overrun.
pub type PacketSink = Arc<dyn Fn(&[i16]) + Send + Sync>;

/// Receives the recording level, once per captured packet.
///
/// # Contract
///
/// Invoked from the audio worker thread at roughly 25 Hz, in the same loop
/// that feeds the transcription providers. It must not block, must not take a
/// lock the audio path also takes, and must not do unbounded work: hand the
/// value to a channel or an atomic and return. A sink that stalls stalls the
/// packets behind it.
pub type LevelSink = Arc<dyn Fn(f32) + Send + Sync>;

pub trait AudioCapture: Send + Sync {
    fn list_devices(&self) -> Result<Vec<InputDevice>>;

    /// Open the device and begin discarding samples, so the first recording
    /// does not pay the 100-300 ms device-open cost.
    ///
    /// On macOS this lights the system microphone indicator for as long as it
    /// is held, which is why it is opt-in rather than the default.
    fn prewarm(&self) -> Result<()>;

    /// Close a pre-warmed stream.
    fn release(&self) -> Result<()>;

    /// Begin accumulating audio.
    fn start(&self) -> Result<StartOutcome>;

    /// Stop and return the recording as packets of
    /// [`wl_core::consts::CHUNK_SAMPLES`] samples each.
    fn stop(&self) -> Vec<Vec<i16>>;

    fn is_recording(&self) -> bool;

    /// Faults observed since the last call. Non-blocking.
    fn take_faults(&self) -> Vec<CaptureFault>;

    /// Select the input device; `None` means follow the system default.
    fn set_device(&self, id: Option<&str>) -> Result<()>;

    /// Install the sink that receives the recording level, or `None` to clear
    /// it.
    ///
    /// Levels are published only while a recording is in progress; a
    /// pre-warmed stream discards its samples and reports nothing. Clearing
    /// the sink when the meter is off-screen releases whatever it captured.
    fn set_level_sink(&self, sink: Option<LevelSink>);

    /// Install the live packet sink, or `None` to clear it.
    ///
    /// Packets are also retained for retries and crash recovery. This sink is
    /// the real-time path to a streaming transcription session.
    fn set_packet_sink(&self, sink: Option<PacketSink>);
}
