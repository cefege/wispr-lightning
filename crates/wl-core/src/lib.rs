//! Platform- and UI-agnostic domain logic for Wispr Lightning.
//!
//! Nothing in this crate may reference an operating-system API, a webview, or a
//! network client. That constraint is what lets the parity suite run unchanged
//! on every target, and it is load-bearing — see `docs/PORT_PLAN.md` §5.

pub mod audio;
pub mod db;
pub mod fsm;
pub mod paths;
pub mod settings;
pub mod text;
pub mod wav;

/// Shared audio constants. Changing these requires corresponding capture and
/// Deepgram streaming changes.
pub mod consts {
    pub const SAMPLE_RATE: u32 = 16_000;
    /// Capture channel count.
    pub const CHANNELS: u16 = 1;
    /// Duration of one audio packet in milliseconds.
    pub const CHUNK_DURATION_MS: u32 = 40;
    /// Samples per packet: 16000 * 40 / 1000.
    pub const CHUNK_SAMPLES: usize = (SAMPLE_RATE as usize) * (CHUNK_DURATION_MS as usize) / 1000;
    /// Bytes per packet: one `i16` per sample.
    pub const CHUNK_BYTES: usize = CHUNK_SAMPLES * 2;
    /// Packet duration as sent in the `append` frame.
    pub const PACKET_DURATION_SECS: f64 = CHUNK_DURATION_MS as f64 / 1000.0;

    /// Hard stop for a single recording.
    pub const MAX_RECORDING_SECS: u64 = 600;
    /// First on-screen warning.
    pub const WARNING_SECS: u64 = 540;
    /// Second, more urgent warning.
    pub const FINAL_WARNING_SECS: u64 = 570;

    /// Recordings shorter than this are discarded without a network round trip.
    pub const MIN_PACKETS: usize = 5;
}
