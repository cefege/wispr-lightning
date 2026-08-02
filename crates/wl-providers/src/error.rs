//! Provider errors.
//!
//! The retryability classification is behavior, not bookkeeping: the pipeline
//! auto-retries twice with a 1.5 s delay for retryable failures and goes
//! straight to the persistent recovery UI otherwise. The user-facing strings
//! are carried over verbatim from the Swift implementation.

use thiserror::Error;

pub type Result<T, E = ProviderError> = std::result::Result<T, E>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// Credentials were rejected.
    ///
    /// `detail` carries a provider-specific actionable message when available,
    /// such as Deepgram's response to a rejected API key.
    #[error("{}", detail.as_deref().unwrap_or("Authentication failed \u{2014} check your Deepgram API key"))]
    AuthFailed { detail: Option<String> },

    #[error("Connection failed \u{2014} check your network")]
    ConnectionFailed,

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Request timed out \u{2014} server did not respond")]
    Timeout,

    #[error("No transcription returned")]
    EmptyResult,

    /// The provider is selected but has no usable credentials yet.
    #[error("{provider} is not configured \u{2014} add an API key in Settings")]
    NotConfigured { provider: &'static str },

    /// The account has no credits left. Retrying cannot fix this and, on a
    /// metered plan, is not free — so it is deliberately not retryable.
    #[error("Out of credits \u{2014} check your {provider} account")]
    QuotaExceeded { provider: &'static str },

    /// Too many requests in the window. Distinct from [`Self::QuotaExceeded`]
    /// because this one always clears on its own: refusing to retry it would
    /// strand a recording that would have succeeded a second later.
    #[error("Rate limited \u{2014} try again in a moment")]
    RateLimited { provider: &'static str },
}

impl ProviderError {
    /// Credentials rejected, with no vendor-specific guidance to offer.
    pub fn auth_failed() -> Self {
        Self::AuthFailed { detail: None }
    }

    /// Credentials rejected, with an actionable message for this vendor.
    pub fn auth_failed_with(detail: impl Into<String>) -> Self {
        Self::AuthFailed {
            detail: Some(detail.into()),
        }
    }

    /// Whether an automatic retry is worth attempting.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::ConnectionFailed
            | Self::ServerError(_)
            | Self::Timeout
            | Self::RateLimited { .. } => true,
            // Retrying a bad key or an empty recording just wastes the user's
            // time, and retrying an exhausted quota wastes their money.
            Self::AuthFailed { .. }
            | Self::EmptyResult
            | Self::NotConfigured { .. }
            | Self::QuotaExceeded { .. } => false,
        }
    }

    /// Text shown in the recording overlay.
    pub fn user_message(&self) -> String {
        self.to_string()
    }

    /// Classify an HTTP status into a provider error.
    pub fn from_status(status: u16, provider: &'static str, body: &str) -> Self {
        match status {
            401 | 403 => Self::auth_failed(),
            402 => Self::QuotaExceeded { provider },
            429 => Self::RateLimited { provider },
            408 | 504 => Self::Timeout,
            _ => Self::ServerError(if body.is_empty() {
                status.to_string()
            } else {
                // Keep the overlay readable; the full body goes to the log.
                body.chars().take(200).collect()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_failures_are_retried() {
        assert!(ProviderError::ConnectionFailed.is_retryable());
        assert!(ProviderError::Timeout.is_retryable());
        assert!(ProviderError::ServerError("boom".into()).is_retryable());
    }

    #[test]
    fn credential_and_quota_failures_are_not_retried() {
        assert!(!ProviderError::auth_failed().is_retryable());
        assert!(!ProviderError::EmptyResult.is_retryable());
        assert!(!ProviderError::NotConfigured {
            provider: "Deepgram"
        }
        .is_retryable());
        assert!(
            !ProviderError::QuotaExceeded {
                provider: "Deepgram"
            }
            .is_retryable(),
            "retrying an exhausted quota cannot succeed and costs the user money"
        );
    }

    #[test]
    fn rate_limiting_is_retried_because_it_clears_on_its_own() {
        // The distinction from QuotaExceeded is the whole point of the split:
        // 429 always clears with backoff, 402 never does.
        assert!(ProviderError::RateLimited {
            provider: "Deepgram"
        }
        .is_retryable());
        assert_ne!(
            ProviderError::RateLimited {
                provider: "Deepgram"
            },
            ProviderError::QuotaExceeded {
                provider: "Deepgram"
            }
        );
    }

    #[test]
    fn user_messages_are_actionable_and_stable() {
        assert_eq!(
            ProviderError::auth_failed().user_message(),
            "Authentication failed \u{2014} check your Deepgram API key"
        );
        assert_eq!(
            ProviderError::ConnectionFailed.user_message(),
            "Connection failed \u{2014} check your network"
        );
        assert_eq!(
            ProviderError::Timeout.user_message(),
            "Request timed out \u{2014} server did not respond"
        );
        assert_eq!(
            ProviderError::EmptyResult.user_message(),
            "No transcription returned"
        );
        assert_eq!(
            ProviderError::ServerError("bad".into()).user_message(),
            "Server error: bad"
        );
    }

    #[test]
    fn http_statuses_map_to_the_right_classification() {
        assert_eq!(
            ProviderError::from_status(401, "Deepgram", ""),
            ProviderError::auth_failed()
        );
        assert_eq!(
            ProviderError::from_status(403, "Deepgram", ""),
            ProviderError::auth_failed()
        );
        assert_eq!(
            ProviderError::from_status(402, "Deepgram", ""),
            ProviderError::QuotaExceeded {
                provider: "Deepgram"
            }
        );
        assert_eq!(
            ProviderError::from_status(429, "Deepgram", ""),
            ProviderError::RateLimited {
                provider: "Deepgram"
            },
            "429 is transient and must not be conflated with an exhausted quota"
        );
        assert_eq!(
            ProviderError::from_status(504, "Deepgram", ""),
            ProviderError::Timeout
        );
        assert_eq!(
            ProviderError::from_status(500, "Deepgram", "upstream exploded"),
            ProviderError::ServerError("upstream exploded".into())
        );
    }
    /// A provider-supplied authentication detail should remain actionable;
    /// otherwise the stable generic message is used.
    #[test]
    fn an_actionable_auth_message_reaches_the_user_verbatim() {
        let actionable = "Deepgram rejected this API key.";
        assert_eq!(
            ProviderError::auth_failed_with(actionable).user_message(),
            actionable
        );
        assert_eq!(
            ProviderError::auth_failed().user_message(),
            "Authentication failed \u{2014} check your Deepgram API key"
        );

        // The detail changes the text, never retryability.
        for e in [
            ProviderError::auth_failed(),
            ProviderError::auth_failed_with("x"),
        ] {
            assert!(
                !e.is_retryable(),
                "a rejected credential never retries in place"
            );
        }
    }

    #[test]
    fn a_server_error_body_is_truncated_for_the_overlay() {
        let long = "x".repeat(5000);
        let ProviderError::ServerError(msg) = ProviderError::from_status(500, "Wispr", &long)
        else {
            panic!("expected a server error");
        };
        assert_eq!(msg.chars().count(), 200);
    }

    #[test]
    fn an_empty_body_falls_back_to_the_status_code() {
        assert_eq!(
            ProviderError::from_status(503, "Wispr", ""),
            ProviderError::ServerError("503".into())
        );
    }
}
