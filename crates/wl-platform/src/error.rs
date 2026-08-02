//! Errors from the platform layer.

use thiserror::Error;

pub type Result<T, E = PlatformError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum PlatformError {
    /// The OS refused: a TCC denial on macOS, a privacy setting on Windows.
    #[error("permission denied: {0}")]
    PermissionDenied(&'static str),

    /// The capability exists in the trait but not on this OS or OS version.
    #[error("unsupported on this platform: {0}")]
    Unsupported(&'static str),

    /// An accessibility or automation query exceeded its deadline. Common when
    /// the target application is busy; recoverable and never fatal.
    #[error("timed out talking to {0}")]
    Timeout(&'static str),

    #[error("no audio input device available")]
    NoInputDevice,

    #[error("audio device unavailable: {0}")]
    AudioDevice(String),

    #[error("clipboard error: {0}")]
    Clipboard(String),

    #[error("input synthesis failed: {0}")]
    InputSynthesis(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl PlatformError {
    /// Whether retrying the same call could plausibly succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Timeout(_) | Self::AudioDevice(_) | Self::Clipboard(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_and_support_failures_are_not_worth_retrying() {
        assert!(!PlatformError::PermissionDenied("microphone").is_transient());
        assert!(!PlatformError::Unsupported("ocr").is_transient());
        assert!(!PlatformError::NoInputDevice.is_transient());
    }

    #[test]
    fn timeouts_and_device_hiccups_are_transient() {
        assert!(PlatformError::Timeout("accessibility").is_transient());
        assert!(PlatformError::AudioDevice("unplugged".into()).is_transient());
    }

    #[test]
    fn messages_name_the_subject() {
        assert_eq!(
            PlatformError::PermissionDenied("microphone").to_string(),
            "permission denied: microphone"
        );
    }
}
