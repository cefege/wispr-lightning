//! The pipeline's view of the user interface.
//!
//! An indirection, not ceremony: the recording pipeline is the most
//! behaviour-dense part of the app and must be testable end to end without a
//! webview, a display server, or a real hotkey. Everything the pipeline needs
//! to *show* goes through this trait, so tests can assert on a transcript of
//! UI calls instead of screenshotting a window.

/// What the recording overlay is currently displaying.
/// The default externally-tagged serde representation is exactly the wire
/// shape the frontend expects: `"Hidden"`, `{"Retrying":{"attempt":1,"of":3}}`.
/// Do not add serde attributes here without updating `ui/src/lib/ipc.ts`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OverlayState {
    Hidden,
    /// Push-to-talk: recording for as long as the key is held.
    Recording,
    /// Hands-free: recording until the next press.
    Locked,
    Processing,
    /// Text is being written into the focused app: instant on the clipboard
    /// path, seconds in Natural Mode at a slow speed.
    ///
    /// The pipeline must enter this state immediately before *every* injection
    /// call. It is not cosmetic: without it the pill keeps whatever it was
    /// showing, so a Retrying yellow tint or a row of error buttons sits on
    /// screen while the text lands.
    Inserting,
    /// Auto-retry in progress, showing which attempt.
    Retrying {
        attempt: u32,
        of: u32,
    },
    /// Transient failure message that dismisses itself.
    Error {
        message: String,
    },
    /// Failure with the audio preserved, offering Retry / Save / Dismiss.
    Recoverable {
        message: String,
    },
}

/// Elapsed-time readout, hidden for the first 30 seconds of a recording.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Elapsed {
    pub label: Option<String>,
    /// 0 = none, 1 = approaching the limit, 2 = about to be cut off.
    pub warning: u8,
}

pub trait Ui: Send + Sync {
    fn set_overlay(&self, state: OverlayState);
    fn set_elapsed(&self, elapsed: Elapsed);

    /// Publish a 0.0–1.0 normalized microphone level for the pill's VU strip.
    ///
    /// Called at ~25 Hz for the whole of a recording, so implementations must
    /// be cheap and must never touch window geometry — a resize at that rate
    /// would be a visible stutter. Publishing a value is all this does.
    fn set_level(&self, level: f32);

    /// Swap the tray icon between idle and recording.
    fn set_recording_indicator(&self, recording: bool);

    /// Update the "last dictation" preview in the tray menu.
    fn set_last_transcription(&self, text: &str);

    /// Tell an open window something changed underneath it, e.g. the
    /// microphone list after a device was unplugged.
    fn notify_changed(&self, topic: &str);
}
