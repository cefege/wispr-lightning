//! Deepgram streaming transcription and local transcript processing.
//!
//! The provider trait remains as the seam used by the recording pipeline's
//! deterministic test doubles. Production has one constructor and one
//! provider: Deepgram.
pub mod credentials;
pub mod deepgram;
pub mod error;
pub mod postprocess;

use async_trait::async_trait;
use std::collections::BTreeMap;

pub use error::{ProviderError, Result};
use wl_core::settings::Settings;

/// The user's dictionary, shaped for transmission.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DictionaryContext {
    /// Phrases to bias recognition toward, most-used first.
    pub vocabulary: Vec<String>,
    /// Spoken form to written form.
    pub replacements: BTreeMap<String, String>,
    /// Trigger phrase to expanded text.
    pub snippets: BTreeMap<String, String>,
}

/// Everything Deepgram needs to turn one recording into text, minus the audio.
#[derive(Debug, Clone, Default)]
pub struct DictationContext {
    pub app: AppContext,
    /// Lines read from the screen, when screen context is enabled.
    pub ocr_context: Vec<String>,
    /// Existing content of the focused text field.
    pub ax_context: Vec<String>,
    pub dictionary: DictionaryContext,
    /// Stable id reused across retries so the backend can deduplicate.
    pub transcript_id: String,
}

/// The focused application, as Deepgram sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppContext {
    pub name: String,
    pub bundle_id: String,
    /// Lowercase category: `messaging`, `email`, `ai`, `other`.
    pub kind: String,
    pub url: String,
}

/// A completed transcription.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptResult {
    pub id: String,
    /// Raw recognizer output, before any formatting.
    pub asr_text: Option<String>,
    /// Formatted output, if the provider or the post-processor produced one.
    pub formatted_text: Option<String>,
    pub duration_secs: f64,
    pub num_words: usize,
}

impl TranscriptResult {
    /// The text to actually insert: formatted when available, raw otherwise.
    pub fn display_text(&self) -> &str {
        self.formatted_text
            .as_deref()
            .or(self.asr_text.as_deref())
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.display_text().is_empty()
    }
}

/// A live Deepgram dictation, from the moment recording starts until the final
/// transcript arrives.
#[async_trait]
pub trait DictationSession: Send + Sync {
    /// Hand over one packet of 16 kHz mono `i16` samples.
    ///
    /// Called from the audio path, so it MUST NOT block, allocate
    /// unpredictably, or await. The implementation pushes onto a channel
    /// drained by the WebSocket worker.
    fn feed(&self, packet: &[i16]);

    /// Close the stream and produce the transcript.
    ///
    /// `ctx` is available at both connection start and finish. Deepgram needs
    /// it at start because keyterms are fixed in the WebSocket URL.
    async fn finish(self: Box<Self>, ctx: &DictationContext) -> Result<TranscriptResult>;

    /// Abandon the dictation. Must not produce a transcript and must release
    /// the connection.
    fn cancel(self: Box<Self>);
}

#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    /// Open a dictation. Called at hotkey press, before any audio exists.
    async fn start(&self, ctx: &DictationContext) -> Result<Box<dyn DictationSession>>;

    /// Verify credentials and reachability, for the settings screen. The only
    /// place a provider is allowed to be slow.
    async fn health(&self) -> Result<()>;

    /// Whether this provider could transcribe right now, judged without any
    /// network call or user-visible prompt.
    ///
    /// This is what the pipeline consults before it opens the microphone, so
    /// it must be cheap and must never block. Contrast with [`Self::health`],
    /// which is allowed to be slow because a human asked it to run.
    ///
    /// Defaults to `true` for deterministic test doubles. Production Deepgram
    /// overrides it with a local credential check.
    fn is_ready(&self) -> bool {
        true
    }

    /// Discard any per-recording caches.
    fn reset(&self) {}
}

/// Construct the application's sole production provider.
pub fn build(settings: &Settings) -> std::sync::Arc<dyn TranscriptionProvider> {
    std::sync::Arc::new(deepgram::DeepgramProvider::new(
        deepgram::DeepgramConfig::from_settings(settings),
    ))
}

/// Whether a usable Deepgram key exists without making a network request.
pub fn is_ready(credentials: &credentials::CredentialStore) -> bool {
    std::env::var(deepgram::API_KEY_ENV).is_ok_and(|value| !value.trim().is_empty())
        || credentials
            .get(credentials::DEEPGRAM_API_KEY)
            .ok()
            .flatten()
            .is_some_and(|key| !key.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> credentials::CredentialStore {
        credentials::CredentialStore::file_backed(
            std::env::temp_dir().join(format!("wl-is-ready-{}.json", uuid::Uuid::new_v4())),
        )
    }

    #[test]
    fn readiness_tracks_the_deepgram_key() {
        let store = store();
        assert!(!is_ready(&store));
        store
            .set(credentials::DEEPGRAM_API_KEY, "dg-test")
            .expect("save key");
        assert!(is_ready(&store));
    }
}
