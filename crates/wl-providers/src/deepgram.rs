//! Deepgram, via the `/v1/listen` streaming WebSocket.
//!
//! The app opens `wss://api.deepgram.com/v1/listen` at hotkey press and pushes
//! headerless s16le PCM as the user speaks. The provider is modeled as a live
//! [`crate::DictationSession`]: the WebSocket receives packets as they arrive
//! and finalizes when the user releases the key.
//!
//! Streaming also removes the batch path's worst property: the upload could
//! not begin until the user let go of the hotkey, so a ten-minute recording
//! paid ~19 MB of transfer *after* the utterance ended. Here the audio is
//! already at Deepgram by the time `Finalize` is sent, and the only remaining
//! wait is the server draining its own buffer.
//!
//! **No prewarm.** The connect URL carries the language and the entire keyterm
//! list, so a socket opened before the dictation context exists is the wrong
//! socket. Deepgram's upgrade is a single TLS handshake against a CDN edge;
//! there is nothing to warm that would survive being re-parameterised.
//!
//! **No pre-open packet buffering.** Swift needs it because `start()` is fire
//! and forget and `feed(packet:)` can arrive before the handshake completes.
//! Here [`TranscriptionProvider::start`] is `async` and yields the session only
//! once the socket is open, so the first [`DictationSession::feed`] physically
//! cannot precede it.
//!
//! Deepgram does no formatting beyond `smart_format`, so this module runs
//! [`crate::postprocess`] locally. Without it, replacements and snippets in the
//! user's dictionary would silently stop working.

use async_trait::async_trait;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use wl_core::consts::{CHANNELS, PACKET_DURATION_SECS, SAMPLE_RATE};
use wl_core::settings::Settings;
use wl_core::text::{select_keyterms, word_count};

use crate::credentials::{CredentialStore, DEEPGRAM_API_KEY};
use crate::error::{ProviderError, Result};
use crate::postprocess::{self, PostProcessOptions};
use crate::{DictationContext, DictationSession, TranscriptResult, TranscriptionProvider};

/// Name used in user-facing error text.
const PROVIDER: &str = "Deepgram";

/// Deepgram's hard ceiling: a request whose keyterms exceed 500 tokens in
/// total is rejected with `Keyterm limit exceeded`.
pub const KEYTERM_MAX_TOKENS: usize = 500;

/// Client-side cap, matching Swift. Well under [`KEYTERM_MAX_TOKENS`] and
/// deliberately so: Deepgram's own guidance is to "stay well under the 500
/// token limit; focus on the most important 20-50 terms", because a long
/// keyterm list dilutes the boost across terms the user never says.
pub const KEYTERM_MAX_PHRASES: usize = 50;

/// REST origin, used only by [`TranscriptionProvider::health`].
pub const DEFAULT_BASE_URL: &str = "https://api.deepgram.com";

/// WebSocket origin for the dictation stream.
pub const DEFAULT_STREAM_URL: &str = "wss://api.deepgram.com";

/// `nova-3` is the only model family that supports `keyterm` prompting, which
/// the user dictionary depends on.
pub const DEFAULT_MODEL: &str = "nova-3";

/// Deepgram's documented default when `language` is omitted.
pub const DEFAULT_LANGUAGE: &str = "en";

/// The code-switching pseudo-language. Deepgram's documented answer for
/// multi-language *streaming*, since detection is pre-recorded only.
pub const MULTILINGUAL: &str = "multi";

/// Settings sentinel for "Auto-detect", matching Swift's
/// `DeepgramLanguage.autoDetectCode`.
pub const AUTO_DETECT: &str = "__auto__";

/// Settings sentinel for "Multilingual (code-switching)", matching Swift's
/// `DeepgramLanguage.multiCode`.
pub const MULTI_SELECT: &str = "__multi__";

/// Legacy auto-detect spelling accepted during settings migration.
const SHARED_AUTO_DETECT: &str = "auto";

/// Environment override for the API key, useful for CI and disposable runs.
pub const API_KEY_ENV: &str = "WISPR_LIGHTNING_DEEPGRAM_KEY";

/// Ceiling on the WebSocket upgrade, matching Swift's
/// `request.timeoutInterval = 30`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// The settings screen blocks on `health()`, so it fails fast.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(10);

/// Keep-alive cadence. Well inside Deepgram's 10 s idle window.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);

/// Skip the keep-alive when audio was sent this recently — during active
/// dictation the timer is a no-op, which is the common case.
const KEEPALIVE_IDLE: Duration = Duration::from_millis(4_500);

/// How long to wait for the `from_finalize` frame after asking Deepgram to
/// drain. Past this the transcript we already have is better than none.
const FINALIZE_TIMEOUT: Duration = Duration::from_secs(3);

/// Grace period between `CloseStream` and dropping the socket, so the server
/// can flush its close frame. Tearing down sooner surfaces as a spurious
/// WebSocket error in the read loop.
const CLOSE_GRACE: Duration = Duration::from_millis(200);

/// Drain request: Deepgram transcribes any buffered audio and answers with a
/// final `Results` frame carrying `from_finalize: true`.
const FINALIZE_FRAME: &str = r#"{"type":"Finalize"}"#;

/// Half-close request: no more audio is coming.
const CLOSE_FRAME: &str = r#"{"type":"CloseStream"}"#;

const KEEPALIVE_FRAME: &str = r#"{"type":"KeepAlive"}"#;

/// Everything the provider needs that comes from settings.
///
/// Deliberately holds no credential. The key is resolved lazily from the local
/// credentials file in [`TranscriptionProvider::start`] and
/// [`TranscriptionProvider::health`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepgramConfig {
    /// Explicit key, bypassing the environment and the credential store.
    /// Normally `None`; set by tests and by callers that already hold a key.
    pub api_key: Option<String>,
    pub model: String,
    /// Language selection. See [`LanguageMode`] for why this is not simply a
    /// string.
    pub language: LanguageMode,
    /// Send the user's vocabulary as `keyterm` recognition hints.
    pub keyterm_boost: bool,
    /// Convert spoken punctuation and layout commands when the configured
    /// language is English. Deepgram calls this its `dictation` feature.
    pub dictation: bool,
    /// Local formatting policy; Deepgram has no server-side formatter.
    pub post_process: PostProcessOptions,
    /// REST origin for `health()`. Overridable so tests can point at a local
    /// server.
    pub base_url: String,
    /// WebSocket origin for the dictation stream.
    pub stream_url: String,
}

/// Why keyterm boosting might not reach the recognizer.
///
/// The request is sent unchanged; this exists so the log can name an
/// incompatible model rather than silently accepting an ineffective setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeytermRisk {
    /// Boosting is off, or it is on and nothing threatens it.
    None,
    /// The requested model is outside the nova-3 family, which is the only
    /// family with `keyterm` prompting at all — so the keyterms are ignored
    /// outright, with or without auto-detect. Reachable because the settings UI
    /// greys the switch out on a non-nova-3 model without writing `false`, so a
    /// settings file can hold `deepgramKeytermBoost: true` on nova-2.
    ModelLacksKeyterm,
}

/// Whether `model` belongs to the nova-3 family, the only family supporting
/// `keyterm` prompting.
///
/// A **prefix** test, not an equality test: `nova-3-general` and
/// `nova-3-medical` are legitimate model ids that do support keyterm, and an
/// exact comparison against `"nova-3"` wrongly excludes them. The trailing
/// hyphen matters — it keeps a hypothetical unrelated `nova-30` out.
pub fn is_nova3_family(model: &str) -> bool {
    let m = model.trim();
    m.eq_ignore_ascii_case(DEFAULT_MODEL)
        || m.len() > DEFAULT_MODEL.len()
            && m[..DEFAULT_MODEL.len()].eq_ignore_ascii_case(DEFAULT_MODEL)
            && m.as_bytes()[DEFAULT_MODEL.len()] == b'-'
}

impl DeepgramConfig {
    /// Read the complete Deepgram request policy from settings.
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            api_key: None,
            model: if settings.deepgram_model.trim().is_empty() {
                DEFAULT_MODEL.to_string()
            } else {
                settings.deepgram_model.clone()
            },
            language: language_mode_for(&settings.deepgram_language),
            keyterm_boost: settings.deepgram_keyterm_boost,
            dictation: settings.command_mode_enabled,
            post_process: PostProcessOptions::default(),
            base_url: DEFAULT_BASE_URL.to_string(),
            stream_url: DEFAULT_STREAM_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_stream_url(mut self, stream_url: impl Into<String>) -> Self {
        self.stream_url = stream_url.into();
        self
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Why this request's keyterms might do nothing.
    ///
    /// Multilingual Nova-3 supports keyterms, so language mode does not affect
    /// this result. A configured model outside the Nova-3 family is the only
    /// incompatible case.
    pub fn keyterm_risk(&self) -> KeytermRisk {
        if self.keyterm_boost && !is_nova3_family(&self.model) {
            KeytermRisk::ModelLacksKeyterm
        } else {
            KeytermRisk::None
        }
    }
}

impl Default for DeepgramConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            model: DEFAULT_MODEL.to_string(),
            language: LanguageMode::Explicit(DEFAULT_LANGUAGE.to_string()),
            keyterm_boost: true,
            dictation: true,
            post_process: PostProcessOptions::default(),
            base_url: DEFAULT_BASE_URL.to_string(),
            stream_url: DEFAULT_STREAM_URL.to_string(),
        }
    }
}

/// How the configured language list is expressed on the wire.
///
/// Deepgram accepts **one** `language`, and its auto-detection is a separate
/// parameter rather than a magic language tag — so this cannot be a plain
/// string without either sending `language=auto` (which Deepgram does not
/// recognize and does not reject) or losing auto-detect entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageMode {
    /// `language=<tag>`, a BCP-47 tag or the `multi` pseudo-language.
    Explicit(String),
    /// The user asked Deepgram to work out the language. On the pre-recorded
    /// endpoint that is `detect_language=true`; see [`apply_language`] for
    /// what it becomes on the streaming endpoint, which is not the same thing.
    Detect,
}

/// Deepgram documents spoken dictation commands for English only. Do not send
/// an attractive no-op for multilingual or non-English requests.
fn supports_dictation(mode: &LanguageMode) -> bool {
    matches!(
        mode,
        LanguageMode::Explicit(tag)
            if tag.eq_ignore_ascii_case("en")
                || tag.get(..3).is_some_and(|prefix| prefix.eq_ignore_ascii_case("en-"))
    )
}

/// Translate one of the app's language codes into the tag Deepgram expects.
///
/// The UI retains legacy short language codes rather than BCP-47, and six of
/// the 104 differ. Passing them through verbatim can select the wrong language
/// while still returning HTTP 200. Verified against Deepgram's Nova 3 table.
///
/// - `engb` → `en-GB`, `dech` → `de-CH`, `yue` → `zh-HK` (Cantonese).
/// - `zhcn` → `zh-Hans`. The dangerous one is bare `zh`: the picker means
///   **Traditional** Chinese by it, while Deepgram's `zh` means *Simplified*.
///   Left alone it silently transcribes into the wrong script, so it maps to
///   `zh-Hant`.
/// - `hien` is Hinglish, which nova-3 has no tag for (only the legacy `nova`
///   and `base` models expose `hi-Latn`). Hinglish is by definition
///   Hindi/English code-switching, and `multi` spans exactly that pair, so it
///   is the closest honest capability rather than a silent downgrade to `hi`.
///
/// Everything else is a plain ISO-639-1 code, which is already a valid BCP-47
/// tag and passes through. Codes nova-3 simply does not support (Welsh, Latin,
/// Maori, Hawaiian and friends) are deliberately *not* remapped: a rejected
/// request the user can act on beats a silent substitution they cannot see.
///
/// The 35 codes in Deepgram's own picker are already BCP-47 and fall through
/// untouched, which is why one function serves both pickers.
///
/// # This function and the picker must agree
///
/// The meaning of each code lives only in the picker label
/// (`ui/src/settings/languages.ts`), and nothing enforces the pair. `zh` is the
/// entry to watch: `languages.ts` names it "Chinese — Traditional (繁體中文)",
/// which is the *only* reason `zh-Hant` is right here. If that table is ever
/// regenerated from an off-the-shelf language list, `zh` will quietly become
/// Simplified and this arm will start producing the wrong script again with no
/// test, status code or health check to catch it.
pub fn deepgram_language_tag(code: &str) -> &str {
    let code = code.trim();
    match code {
        c if c.eq_ignore_ascii_case("engb") => "en-GB",
        c if c.eq_ignore_ascii_case("dech") => "de-CH",
        c if c.eq_ignore_ascii_case("zhcn") => "zh-Hans",
        // Traditional per `ui/src/settings/languages.ts`, NOT Deepgram's `zh`.
        c if c.eq_ignore_ascii_case("zh") => "zh-Hant",
        c if c.eq_ignore_ascii_case("yue") => "zh-HK",
        c if c.eq_ignore_ascii_case("hien") => MULTILINGUAL,
        other => other,
    }
}

/// Recognize a picker sentinel, in either spelling.
///
/// `__auto__` / `__multi__` are Deepgram's own picker; `auto` is the shared
/// picker's. `multi` is accepted too because it is simultaneously a real
/// Deepgram language tag and the thing `__multi__` means, so a settings file
/// holding either must behave the same.
fn sentinel(code: &str) -> Option<LanguageMode> {
    let c = code.trim();
    if c.eq_ignore_ascii_case(AUTO_DETECT) || c.eq_ignore_ascii_case(SHARED_AUTO_DETECT) {
        return Some(LanguageMode::Detect);
    }
    if c.eq_ignore_ascii_case(MULTI_SELECT) || c.eq_ignore_ascii_case(MULTILINGUAL) {
        return Some(LanguageMode::Explicit(MULTILINGUAL.to_string()));
    }
    None
}

/// Interpret Deepgram's own single-value language setting
/// (`settings.deepgramLanguage`).
///
/// Mirrors Swift's `switch settings.deepgramLanguage` exactly: the two
/// sentinels, then a code, with empty falling back to Deepgram's own default.
pub fn language_mode_for(code: &str) -> LanguageMode {
    if let Some(mode) = sentinel(code) {
        return mode;
    }
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return LanguageMode::Explicit(DEFAULT_LANGUAGE.to_string());
    }
    LanguageMode::Explicit(deepgram_language_tag(trimmed).to_string())
}

/// Collapse the *shared* multi-select language list into Deepgram's request
/// parameters.
///
/// - a detect sentinel anywhere → [`LanguageMode::Detect`].
/// - nothing configured → Deepgram's own default `en`.
/// - exactly one → that code, translated by [`deepgram_language_tag`].
/// - two or more → `multi`, the code-switching pseudo-language.
///
/// Detection wins over anything else present, so the result never depends on
/// list order even though the settings model makes the toggle exclusive.
///
/// Used only when the user has never touched Deepgram's own picker; see
/// [`settings_language`].
pub fn language_mode(languages: &[String]) -> LanguageMode {
    if languages
        .iter()
        .any(|l| sentinel(l) == Some(LanguageMode::Detect))
    {
        return LanguageMode::Detect;
    }
    let mut configured = languages.iter().map(|l| l.trim()).filter(|l| !l.is_empty());
    match (configured.next(), configured.next()) {
        (None, _) => LanguageMode::Explicit(DEFAULT_LANGUAGE.to_string()),
        (Some(only), None) => language_mode_for(only),
        (Some(_), Some(_)) => LanguageMode::Explicit(MULTILINGUAL.to_string()),
    }
}

/// Resolve the persisted Deepgram language.
pub fn settings_language(settings: &Settings) -> LanguageMode {
    language_mode_for(&settings.deepgram_language)
}

/// Write the language parameter for `mode` into the streaming query.
///
/// Deepgram explicitly documents that `detect_language` is not supported for
/// streaming and recommends Nova-3 multilingual code-switching instead.
/// Combining `language=multi` with `detect_language=true` causes Deepgram to
/// reject the WebSocket upgrade with HTTP 400, so Detect maps only to the
/// documented `language=multi` streaming contract.
fn apply_language(query: &mut form_urlencoded::Serializer<'_, String>, mode: &LanguageMode) {
    match mode {
        LanguageMode::Explicit(tag) => {
            query.append_pair("language", tag);
        }
        LanguageMode::Detect => {
            tracing::info!(
                "Deepgram does not detect languages on the streaming endpoint; \
                 sending language=multi (code-switching) for this dictation"
            );
            query.append_pair("language", MULTILINGUAL);
        }
    }
}

pub struct DeepgramProvider {
    config: DeepgramConfig,
    store: CredentialStore,
    /// REST client, used only by `health()`.
    client: reqwest::Client,
    /// The stream timings, as fields rather than constants so the finalize and
    /// keep-alive paths are testable in milliseconds instead of seconds.
    /// Production always uses the module constants.
    timings: Timings,
}

#[derive(Debug, Clone, Copy)]
struct Timings {
    finalize: Duration,
    keepalive_interval: Duration,
    keepalive_idle: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            finalize: FINALIZE_TIMEOUT,
            keepalive_interval: KEEPALIVE_INTERVAL,
            keepalive_idle: KEEPALIVE_IDLE,
        }
    }
}

impl DeepgramProvider {
    pub fn new(config: DeepgramConfig) -> Self {
        // `reqwest` uses `rustls-no-provider` so cross-platform builds do not
        // pull in AWS-LC. Installing Ring is process-global and idempotent.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            config,
            store: CredentialStore::new(),
            client: reqwest::Client::new(),
            timings: Timings::default(),
        }
    }

    /// Point the key lookup at an explicit file for deterministic tests.
    pub fn with_credential_store(mut self, store: CredentialStore) -> Self {
        self.store = store;
        self
    }

    pub fn config(&self) -> &DeepgramConfig {
        &self.config
    }

    /// Resolve the API key: explicit override, then the environment, then the
    /// credential store.
    ///
    /// A store read failure is folded into "no key configured": the two are
    /// indistinguishable to the user and have the same remedy.
    fn key(&self) -> Result<String> {
        let stored = match self.store.get(DEEPGRAM_API_KEY) {
            Ok(key) => key,
            Err(e) => {
                tracing::warn!("could not read the Deepgram API key: {e}");
                None
            }
        };
        choose_key(
            self.config.api_key.as_deref(),
            std::env::var(API_KEY_ENV).ok().as_deref(),
            stored.as_deref(),
        )
        .ok_or(ProviderError::NotConfigured { provider: PROVIDER })
    }

    /// Build the fully-qualified `/v1/listen` WebSocket URL for one dictation.
    ///
    /// `form_urlencoded` gives us Deepgram's expected repeated-key form for
    /// `keyterm` and encodes spaces inside a multi-word term as `+`, both of
    /// which the API documents as correct.
    fn listen_url(&self, ctx: &DictationContext) -> String {
        let mode = &self.config.language;
        match self.config.keyterm_risk() {
            KeytermRisk::None => {}
            KeytermRisk::ModelLacksKeyterm => tracing::warn!(
                model = %self.config.model,
                "keyterm boosting is being ignored: keyterm prompting exists only on the \
                 nova-3 family, so the configured vocabulary has no effect on this model"
            ),
        }

        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("model", &self.config.model);
        // The stream is headerless PCM, so these three replace the WAV header
        // the batch path used to send.
        query.append_pair("encoding", "linear16");
        query.append_pair("sample_rate", &SAMPLE_RATE.to_string());
        query.append_pair("channels", &CHANNELS.to_string());
        query.append_pair("smart_format", "true");
        query.append_pair("punctuate", "true");
        // Interims are pure noise for push-to-talk: nothing renders partial
        // text, and disabling them is what lets the parser treat every
        // `is_final` frame as a segment rather than deduplicating.
        query.append_pair("interim_results", "false");

        // Opt every request out of Deepgram's Model Improvement Program.
        //
        // Not a tuning knob — a privacy guarantee. Without it, dictated audio
        // is retained and used to train Deepgram's models, and a dictation app
        // carries whatever the user happens to say: passwords read aloud,
        // medical details, unreleased work. The default is opt-IN, so omitting
        // this parameter silently enrols the user. The Swift implementation
        // sends it on every request and so must this one.
        query.append_pair("mip_opt_out", "true");

        apply_language(&mut query, mode);
        if self.config.dictation && supports_dictation(mode) {
            // `punctuate=true` above is a documented prerequisite. This turns
            // “comma”, “new line”, and “new paragraph” into their written
            // forms instead of leaving the command words in the transcript.
            query.append_pair("dictation", "true");
        }

        if self.config.keyterm_boost {
            for term in keyterms(ctx) {
                query.append_pair("keyterm", &term);
            }
        }

        format!(
            "{}/v1/listen?{}",
            self.config.stream_url.trim_end_matches('/'),
            query.finish()
        )
    }
}

/// The keyterms to actually send, in priority order.
///
/// Deepgram has no arbitrary text-context parameter. Its documented mechanism
/// is repeated `keyterm` parameters, intended for terminology, product names,
/// proper nouns and short phrases. We therefore keep the user's explicit
/// dictionary first, then distil the focused app, accessibility text and OCR
/// into terms that look distinctive rather than sending prose that would
/// dilute recognition. The final selector enforces the documented 500-token
/// ceiling and our 50-phrase quality cap.
fn keyterms(ctx: &DictationContext) -> Vec<String> {
    let mut candidates = ctx.dictionary.vocabulary.clone();
    let mut seen: HashSet<String> = candidates
        .iter()
        .map(|term| term.trim().to_lowercase())
        .collect();

    if !ctx.app.name.trim().is_empty() {
        append_context_terms(&ctx.app.name, &mut candidates, &mut seen);
    }
    for line in ctx.ax_context.iter().chain(&ctx.ocr_context) {
        append_context_terms(line, &mut candidates, &mut seen);
    }

    let mut terms = select_keyterms(&candidates, KEYTERM_MAX_TOKENS);
    terms.truncate(KEYTERM_MAX_PHRASES);
    terms
}

/// Pull proper names and technical identifiers from one context line.
///
/// Lowercase prose is intentionally ignored: Deepgram recommends a focused
/// 20–50-term list, and filling that budget with “the”, “save”, or whole OCR
/// sentences makes the real names less effective. Consecutive title-case words
/// stay together so `Visual Studio Code` remains one phrase.
fn append_context_terms(line: &str, out: &mut Vec<String>, seen: &mut HashSet<String>) {
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| {
                !(c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+' | '#' | '@'))
            })
        })
        .filter(|word| !word.is_empty())
        .collect();

    let mut start = 0;
    while start < words.len() {
        if !is_distinctive_context_word(words[start]) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < words.len() && end - start < 4 && is_distinctive_context_word(words[end]) {
            end += 1;
        }
        let phrase = words[start..end].join(" ");
        let normalized = phrase.to_lowercase();
        if seen.insert(normalized) {
            out.push(phrase);
        }
        start = end;
    }
}

fn is_distinctive_context_word(word: &str) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let has_digit = word.chars().any(|c| c.is_ascii_digit());
    if word.chars().count() < 3 && !has_digit {
        return false;
    }
    first.is_uppercase()
        || chars.any(char::is_uppercase)
        || has_digit
        || word
            .chars()
            .any(|c| matches!(c, '_' | '.' | '/' | '+' | '#' | '@'))
}

/// API key precedence, split out from the I/O so it is testable without
/// mutating the process environment.
///
/// The environment beats the stored key so a developer or CI run can override
/// whatever the user has saved, matching Swift.
fn choose_key(explicit: Option<&str>, env: Option<&str>, stored: Option<&str>) -> Option<String> {
    [explicit, env, stored]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|k| !k.is_empty())
        .map(str::to_owned)
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Sink = SplitSink<Socket, Message>;

/// A message for the socket-owning task.
enum Outbound {
    /// One packet of s16le PCM, already byte-encoded.
    Audio(Vec<u8>),
    /// The user released the hotkey: drain and complete.
    Finalize,
    /// Abandon the dictation without producing a transcript.
    Cancel,
}

/// What the stream produced, before post-processing.
#[derive(Debug, Default, PartialEq, Eq)]
struct StreamOutcome {
    /// Every `is_final` transcript, in arrival order.
    segments: Vec<String>,
    /// The language Deepgram reported, when it reported one.
    detected_language: Option<String>,
}

#[async_trait]
impl TranscriptionProvider for DeepgramProvider {
    fn is_ready(&self) -> bool {
        // Production keys are resolved lazily from the local credentials file;
        // `config.api_key` is normally test-only.
        self.key().is_ok()
    }

    /// Open the socket. Called at hotkey press, before any audio exists.
    async fn start(&self, ctx: &DictationContext) -> Result<Box<dyn DictationSession>> {
        let key = self.key()?;
        let url = self.listen_url(ctx);

        let mut request = url.as_str().into_client_request().map_err(|err| {
            tracing::warn!(%err, "Deepgram endpoint is not a usable URL");
            ProviderError::ConnectionFailed
        })?;
        let credential = HeaderValue::from_str(&format!("Token {key}")).map_err(|_| {
            // A key with a newline or a non-ASCII byte cannot go in a header.
            // That is a broken saved key, not a transport problem.
            tracing::warn!("the saved Deepgram API key is not a valid HTTP header value");
            ProviderError::auth_failed_with(
                "Deepgram: the saved API key contains invalid characters. Paste a fresh one \
                 from console.deepgram.com.",
            )
        })?;
        request.headers_mut().insert("Authorization", credential);

        let socket = match tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request)).await {
            Err(_elapsed) => {
                tracing::warn!("Deepgram did not complete the WebSocket upgrade in time");
                return Err(ProviderError::Timeout);
            }
            Ok(Err(err)) => return Err(connect_error(err)),
            Ok(Ok((socket, _response))) => socket,
        };
        tracing::debug!(model = %self.config.model, "Deepgram: stream opened");

        let (outbound, inbox) = mpsc::unbounded_channel();
        let (done, outcome) = oneshot::channel();
        tokio::spawn(drive(socket, inbox, done, self.timings));

        Ok(Box::new(DeepgramSession {
            outbound,
            outcome,
            packets: AtomicUsize::new(0),
            post_process: self.config.post_process,
        }))
    }

    /// `GET /v1/auth/token` is Deepgram's documented key-validation request:
    /// it returns the key's own details when valid and `invalid credentials`
    /// otherwise, and transcribes nothing.
    async fn health(&self) -> Result<()> {
        let key = self.key()?;
        let url = format!(
            "{}/v1/auth/token",
            self.config.base_url.trim_end_matches('/')
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Token {key}"))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .map_err(transport_error)?;

        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(status_error(status.as_u16(), &body))
    }
}

/// A dictation in flight.
///
/// Owns nothing but channel ends: the socket lives in the task spawned by
/// [`TranscriptionProvider::start`], which is the only way to satisfy both
/// halves of the trait — `feed` is synchronous and called from the audio path,
/// while the socket needs an executor.
struct DeepgramSession {
    outbound: mpsc::UnboundedSender<Outbound>,
    outcome: oneshot::Receiver<Result<StreamOutcome>>,
    /// Packets fed, for the duration estimate. Deepgram's streaming frames
    /// carry per-frame durations but no session total.
    packets: AtomicUsize,
    post_process: PostProcessOptions,
}

#[async_trait]
impl DictationSession for DeepgramSession {
    /// Encode one packet and hand it to the socket task.
    ///
    /// The single 1280-byte allocation is unavoidable — the caller's slice is
    /// borrowed from the audio callback's buffer and cannot outlive it — but
    /// it is bounded and the send is a lock-free enqueue, so the audio thread
    /// never waits on the network.
    fn feed(&self, packet: &[i16]) {
        self.packets.fetch_add(1, Ordering::Relaxed);
        let mut pcm = Vec::with_capacity(packet.len() * 2);
        for sample in packet {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        // A closed channel means the socket task has already finished or
        // failed; `finish` reports why. Dropping audio here is correct.
        let _ = self.outbound.send(Outbound::Audio(pcm));
    }

    async fn finish(self: Box<Self>, ctx: &DictationContext) -> Result<TranscriptResult> {
        // A dead channel is not an error yet: the socket task always resolves
        // the outcome before exiting, so the real reason is waiting for us.
        let _ = self.outbound.send(Outbound::Finalize);

        let outcome = self.outcome.await.map_err(|_| {
            tracing::warn!("the Deepgram socket task ended without reporting an outcome");
            ProviderError::ConnectionFailed
        })??;

        let asr = outcome.segments.join(" ").trim().to_string();
        // Parity with the rest of the pipeline: a recognizer that heard
        // nothing is a failure the user is told about, not a successful empty
        // insertion. `EmptyResult` is also the one error that does not advance
        // the fallback chain — no other vendor will hear what was never said.
        if asr.is_empty() {
            tracing::info!("Deepgram returned no transcript");
            return Err(ProviderError::EmptyResult);
        }

        let formatted = postprocess::format_if_enabled(&asr, &ctx.dictionary, &self.post_process);
        let duration_secs = self.packets.load(Ordering::Relaxed) as f64 * PACKET_DURATION_SECS;

        tracing::debug!(
            detected_language = outcome.detected_language.as_deref().unwrap_or("-"),
            chars = asr.len(),
            duration = duration_secs,
            "Deepgram transcription complete"
        );

        let words = word_count(formatted.as_deref().unwrap_or(&asr));
        Ok(TranscriptResult {
            id: ctx.transcript_id.clone(),
            asr_text: Some(asr),
            formatted_text: formatted,
            duration_secs,
            num_words: words,
        })
    }

    fn cancel(self: Box<Self>) {
        let _ = self.outbound.send(Outbound::Cancel);
    }
}

/// Own the socket for the lifetime of one dictation.
///
/// Everything that races — inbound frames, outbound audio, the keep-alive
/// timer and the finalize deadline — is serialized here, which is what makes
/// Swift's `SafeCompletion` gate unnecessary: `done` is an
/// [`oneshot::Sender`], so the compiler enforces at most one outcome.
///
/// The task **always** resolves `done` before returning, except after
/// [`Outbound::Cancel`] where nobody is listening. That is what lets `finish`
/// treat a dead channel as "the reason is already in the outcome" rather than
/// as a bare connection failure.
async fn drive(
    socket: Socket,
    mut inbox: mpsc::UnboundedReceiver<Outbound>,
    done: oneshot::Sender<Result<StreamOutcome>>,
    timings: Timings,
) {
    let (mut sink, mut source) = socket.split();
    let mut done = Some(done);
    let mut outcome = StreamOutcome::default();
    let mut last_send = Instant::now();
    let mut finalize_at: Option<Instant> = None;
    let mut keepalive = tokio::time::interval_at(
        Instant::now() + timings.keepalive_interval,
        timings.keepalive_interval,
    );
    // A tick missed while the loop was busy is not a tick owed: bursting two
    // KeepAlives back to back tells Deepgram nothing the first one did not.
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            message = inbox.recv() => match message {
                Some(Outbound::Audio(pcm)) => {
                    if sink.send(Message::binary(pcm)).await.is_err() {
                        return fail(&mut done, ProviderError::ConnectionFailed);
                    }
                    last_send = Instant::now();
                }
                Some(Outbound::Finalize) => {
                    if sink.send(Message::text(FINALIZE_FRAME)).await.is_err() {
                        return fail(&mut done, ProviderError::ConnectionFailed);
                    }
                    last_send = Instant::now();
                    finalize_at = Some(Instant::now() + timings.finalize);
                }
                // Cancel, or every sender dropped: leave without a transcript.
                Some(Outbound::Cancel) | None => {
                    tear_down(&mut sink, "cancel").await;
                    return;
                }
            },

            received = source.next() => match received {
                Some(Ok(Message::Text(text))) => {
                    if absorb(&mut outcome, &text) && finalize_at.is_some() {
                        return complete(&mut sink, &mut done, outcome).await;
                    }
                }
                // Deepgram answers in text; a binary frame would be UTF-8 JSON
                // anyway, so decode it rather than discarding a transcript.
                Some(Ok(Message::Binary(bytes))) => {
                    if let Ok(text) = std::str::from_utf8(&bytes) {
                        if absorb(&mut outcome, text) && finalize_at.is_some() {
                            return complete(&mut sink, &mut done, outcome).await;
                        }
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => {
                    tracing::warn!(%err, "Deepgram socket read failed");
                    return fail(&mut done, ProviderError::ConnectionFailed);
                }
                // The server hung up. After Finalize that is just the drain
                // finishing, so keep what we have; before it, the dictation
                // lost its connection mid-sentence.
                None => {
                    return if finalize_at.is_some() {
                        complete(&mut sink, &mut done, outcome).await
                    } else {
                        tracing::warn!("Deepgram closed the stream before the dictation ended");
                        fail(&mut done, ProviderError::ConnectionFailed)
                    };
                }
            },

            _ = tokio::time::sleep_until(finalize_at.unwrap_or_else(Instant::now)),
                if finalize_at.is_some() =>
            {
                tracing::warn!(
                    finalize_timeout = ?timings.finalize,
                    "Deepgram did not answer Finalize in time; \
                     completing with the segments already received"
                );
                return complete(&mut sink, &mut done, outcome).await;
            }

            _ = keepalive.tick() => {
                if last_send.elapsed() >= timings.keepalive_idle
                    && sink.send(Message::text(KEEPALIVE_FRAME)).await.is_err()
                {
                    return fail(&mut done, ProviderError::ConnectionFailed);
                }
            }
        }
    }
}

/// Fold one received frame into the outcome.
///
/// Returns whether this frame was Deepgram's answer to `Finalize`, which is the
/// signal to complete without waiting out the deadline. An *empty* final frame
/// still counts: after `Finalize` on silence that is the only frame we get.
fn absorb(outcome: &mut StreamOutcome, text: &str) -> bool {
    let frame = match serde_json::from_str::<StreamFrame>(text) {
        Ok(frame) => frame,
        Err(err) => {
            tracing::debug!(%err, "ignoring an unrecognized Deepgram frame");
            return false;
        }
    };
    let StreamFrame::Results(results) = frame else {
        return false;
    };
    // With `interim_results=false` these should not arrive at all, but a
    // partial counted as a segment would duplicate the text it precedes.
    if !results.is_final {
        return false;
    }

    let alternative = results.channel.alternatives.first();
    if let Some(transcript) = alternative
        .map(|a| a.transcript.trim())
        .filter(|t| !t.is_empty())
    {
        outcome.segments.push(transcript.to_string());
    }
    if outcome.detected_language.is_none() {
        outcome.detected_language = results.channel.language();
    }
    results.from_finalize
}

/// Deliver the transcript, then close politely.
///
/// The outcome is sent *before* the close handshake: the user is waiting on
/// this text, and Deepgram's 200 ms flush window is latency they should not
/// pay for.
async fn complete(
    sink: &mut Sink,
    done: &mut Option<oneshot::Sender<Result<StreamOutcome>>>,
    outcome: StreamOutcome,
) {
    let Some(gate) = done.take() else {
        return;
    };
    let _ = gate.send(Ok(outcome));

    let _ = sink.send(Message::text(CLOSE_FRAME)).await;
    tokio::time::sleep(CLOSE_GRACE).await;
    tear_down(sink, "complete").await;
}

fn fail(done: &mut Option<oneshot::Sender<Result<StreamOutcome>>>, error: ProviderError) {
    if let Some(gate) = done.take() {
        let _ = gate.send(Err(error));
    }
}

async fn tear_down(sink: &mut Sink, reason: &str) {
    tracing::debug!(reason, "Deepgram: closing the stream");
    let _ = sink.close().await;
}

/// Classify a failed WebSocket upgrade.
///
/// Deepgram reports credential and parameter problems as the HTTP status of the
/// upgrade response, so this is the same taxonomy as the REST path.
fn connect_error(err: tungstenite::Error) -> ProviderError {
    if let tungstenite::Error::Http(response) = &err {
        let body = response
            .body()
            .as_deref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        let status = response.status().as_u16();
        tracing::warn!(status, "Deepgram refused the WebSocket upgrade");
        return status_error(status, &body);
    }
    tracing::warn!(%err, "could not open the Deepgram socket");
    ProviderError::ConnectionFailed
}

/// Map an HTTP status onto a provider error.
///
/// Only 401/403 is specialised, and only because Deepgram's remedy is specific
/// enough to be worth saying: this app has no Deepgram sign-in, so the generic
/// "please sign in again" would send the user looking for a screen that does
/// not exist.
///
/// 400 and 404 stay [`ProviderError::ServerError`]. Swift routes them as an
/// auth failure to make its chain advance without retrying, but that shows the
/// user an authentication message for what is really a bad model or language
/// id. The honest error costs two retries before the chain advances; the
/// dishonest one costs a support ticket.
fn status_error(status: u16, body: &str) -> ProviderError {
    match status {
        401 | 403 => ProviderError::auth_failed_with(format!(
            "Deepgram rejected the API key (HTTP {status}). Open Settings \u{2192} Accounts \
             \u{2192} Deepgram and paste a fresh key from console.deepgram.com."
        )),
        400 | 404 => ProviderError::ServerError(format!(
            "Deepgram rejected the request (HTTP {status}) \u{2014} check the configured model \
             and language"
        )),
        _ => ProviderError::from_status(status, PROVIDER, body),
    }
}

/// Distinguish "we gave up waiting" from "we could not reach them", because
/// the two produce different overlay text.
fn transport_error(e: reqwest::Error) -> ProviderError {
    if e.is_timeout() {
        ProviderError::Timeout
    } else {
        tracing::warn!("Deepgram request failed: {e}");
        ProviderError::ConnectionFailed
    }
}

/// One frame from the streaming endpoint.
///
/// Deliberately *not* shared with the pre-recorded shape: streaming puts the
/// transcript under a bare `channel` and adds `is_final` / `from_finalize`,
/// while pre-recorded nests it under `results.channels[]` and has no
/// finalization at all. One struct for both would silently accept the wrong
/// payload and return an empty transcript for a perfectly good response.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum StreamFrame {
    Results(StreamResults),
    /// `Metadata`, `SpeechStarted`, `UtteranceEnd` and anything Deepgram adds
    /// later. All ignored: the transcript arrives only in `Results`.
    #[serde(other)]
    Ignored,
}

#[derive(Debug, Default, Deserialize)]
struct StreamResults {
    #[serde(default)]
    is_final: bool,
    /// Set on the one frame Deepgram emits in answer to `Finalize`.
    #[serde(default)]
    from_finalize: bool,
    #[serde(default)]
    channel: StreamChannel,
}

#[derive(Debug, Default, Deserialize)]
struct StreamChannel {
    #[serde(default)]
    alternatives: Vec<StreamAlternative>,
    /// Where pre-recorded language detection reports its answer, and where
    /// Swift looks. Never populated by the streaming endpoint today; kept
    /// because it costs one `Option` and is the field that will appear first
    /// if Deepgram ships streaming detection.
    #[serde(default)]
    detected_language: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamAlternative {
    #[serde(default)]
    transcript: String,
    /// Under `language=multi`, the languages heard in this segment, most words
    /// first. This is the documented *streaming* location — on the alternative,
    /// not on the channel.
    #[serde(default)]
    languages: Vec<String>,
}

impl StreamChannel {
    /// The language Deepgram reported for this frame, from either shape.
    fn language(&self) -> Option<String> {
        self.alternatives
            .first()
            .and_then(|a| a.languages.first())
            .or(self.detected_language.as_ref())
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DictionaryContext;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Everything the fake Deepgram saw, in arrival order.
    #[derive(Default)]
    struct Recording {
        query: Mutex<Option<String>>,
        authorization: Mutex<Option<String>>,
        binary: Mutex<Vec<Vec<u8>>>,
        text: Mutex<Vec<String>>,
    }

    impl Recording {
        fn query(&self) -> String {
            self.query
                .lock()
                .expect("query")
                .clone()
                .expect("the client never connected")
        }

        fn text_frames(&self) -> Vec<String> {
            self.text.lock().expect("text").clone()
        }

        fn binary_frames(&self) -> Vec<Vec<u8>> {
            self.binary.lock().expect("binary").clone()
        }
    }

    /// A `Results` frame in Deepgram's streaming shape.
    fn results(transcript: &str, is_final: bool, from_finalize: bool) -> Value {
        json!({
            "type": "Results",
            "channel_index": [0, 1],
            "duration": 1.2,
            "is_final": is_final,
            "speech_final": is_final,
            "channel": { "alternatives": [{ "transcript": transcript, "confidence": 0.99 }] },
            "from_finalize": from_finalize,
        })
    }

    /// The frame Deepgram sends in answer to `Finalize`.
    fn finalized(transcript: &str) -> Value {
        results(transcript, true, true)
    }

    /// A scripted Deepgram streaming endpoint on loopback.
    ///
    /// Replays `on_finalize` when it receives `{"type":"Finalize"}`; an empty
    /// list models a server that never answers the drain.
    async fn start_server(on_finalize: Vec<Value>) -> (String, Arc<Recording>) {
        start_server_with(on_finalize, Vec::new(), None).await
    }

    /// As [`start_server`], but also emitting `on_connect` as soon as the
    /// socket opens, and optionally rejecting the upgrade with `reject`.
    // The handshake callback's `Result<Response, ErrorResponse>` is
    // tungstenite's signature, not ours, and `ErrorResponse` is a whole HTTP
    // response. Boxing it is not an option the trait allows.
    #[allow(clippy::result_large_err)]
    async fn start_server_with(
        on_finalize: Vec<Value>,
        on_connect: Vec<Value>,
        reject: Option<u16>,
    ) -> (String, Arc<Recording>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let recording = Arc::new(Recording::default());

        let sink = Arc::clone(&recording);
        tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let observer = Arc::clone(&sink);
            let handshake = |request: &Request, response: Response| {
                *observer.query.lock().expect("query") =
                    Some(request.uri().query().unwrap_or_default().to_string());
                *observer.authorization.lock().expect("auth") = request
                    .headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                match reject {
                    None => Ok(response),
                    Some(status) => {
                        let mut error = ErrorResponse::new(Some("nope".to_string()));
                        *error.status_mut() =
                            tungstenite::http::StatusCode::from_u16(status).expect("status");
                        Err(error)
                    }
                }
            };

            let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, handshake).await else {
                return;
            };
            for frame in &on_connect {
                if ws.send(Message::text(frame.to_string())).await.is_err() {
                    return;
                }
            }

            while let Some(Ok(message)) = ws.next().await {
                match message {
                    Message::Binary(bytes) => {
                        sink.binary.lock().expect("binary").push(bytes.to_vec());
                    }
                    Message::Text(text) => {
                        let text = text.to_string();
                        sink.text.lock().expect("text").push(text.clone());
                        if text.contains("Finalize") {
                            for frame in &on_finalize {
                                if ws.send(Message::text(frame.to_string())).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Message::Close(_) => return,
                    _ => {}
                }
            }
        });

        (format!("ws://{addr}"), recording)
    }

    /// A provider wired to `url`, with a finalize deadline short enough that
    /// the timeout path does not cost three seconds of test time.
    fn provider(url: &str) -> DeepgramProvider {
        provider_with_finalize(url, Duration::from_millis(300))
    }

    fn provider_with_finalize(url: &str, finalize_timeout: Duration) -> DeepgramProvider {
        let mut provider = DeepgramProvider::new(
            DeepgramConfig::default()
                .with_api_key("test-key")
                .with_stream_url(url),
        )
        .with_credential_store(empty_store());
        provider.timings.finalize = finalize_timeout;
        provider
    }

    /// A store pointed at a path that does not exist, so a developer's saved
    /// key cannot make a missing-credential test pass.
    fn empty_store() -> CredentialStore {
        CredentialStore::file_backed(
            std::env::temp_dir()
                .join(format!("wl-dg-{}", uuid::Uuid::new_v4()))
                .join("credentials.json"),
        )
    }

    fn context(vocabulary: &[&str]) -> DictationContext {
        DictationContext {
            dictionary: DictionaryContext {
                vocabulary: vocabulary.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            transcript_id: "TID-1".into(),
            ..Default::default()
        }
    }

    /// One 640-sample packet of a recognisable ramp, so a frame assertion can
    /// check the payload survived verbatim.
    fn packet() -> Vec<i16> {
        (0..wl_core::consts::CHUNK_SAMPLES as i16).collect()
    }

    /// Run one whole dictation: connect, feed `packets`, finalize.
    async fn dictate(
        provider: &DeepgramProvider,
        ctx: &DictationContext,
        packets: usize,
    ) -> Result<TranscriptResult> {
        let session = provider.start(ctx).await?;
        for _ in 0..packets {
            session.feed(&packet());
        }
        session.finish(ctx).await
    }

    // -- URL ---------------------------------------------------------------

    #[tokio::test]
    async fn the_stream_url_carries_every_parameter_deepgram_needs() {
        let (url, server) = start_server(vec![finalized("hello")]).await;
        dictate(&provider(&url), &context(&[]), 1)
            .await
            .expect("transcribe");

        let query = server.query();
        for expected in [
            "model=nova-3",
            "encoding=linear16",
            "sample_rate=16000",
            "channels=1",
            "smart_format=true",
            "punctuate=true",
            "interim_results=false",
            "mip_opt_out=true",
            "language=en",
            "dictation=true",
        ] {
            assert!(query.contains(expected), "{query} is missing {expected}");
        }
    }

    /// A privacy guarantee, not a preference. Deepgram's Model Improvement
    /// Program defaults to opt-IN, so a missing parameter enrols the user's
    /// dictated audio — which is whatever they happened to say out loud — into
    /// Deepgram's training data. This test exists so nobody can drop the
    /// parameter while refactoring the query builder.
    #[tokio::test]
    async fn every_request_opts_out_of_model_training() {
        for (boost, language) in [
            (true, LanguageMode::Explicit("en".into())),
            (false, LanguageMode::Detect),
            (true, LanguageMode::Explicit(MULTILINGUAL.into())),
            (false, LanguageMode::Explicit("de-CH".into())),
        ] {
            let (url, server) = start_server(vec![finalized("hi")]).await;
            let mut p = provider(&url);
            p.config.keyterm_boost = boost;
            p.config.language = language.clone();

            dictate(&p, &context(&["Kubernetes"]), 1)
                .await
                .expect("transcribe");

            let query = server.query();
            assert!(
                query.contains("mip_opt_out=true"),
                "boost={boost} language={language:?} produced a query with no opt-out: {query}"
            );
        }
    }

    #[tokio::test]
    async fn the_api_key_is_sent_as_a_deepgram_token_credential_on_the_upgrade() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        dictate(&provider(&url), &context(&[]), 1)
            .await
            .expect("transcribe");

        assert_eq!(
            server.authorization.lock().expect("auth").as_deref(),
            Some("Token test-key")
        );
    }

    // -- Keyterms ----------------------------------------------------------

    #[tokio::test]
    async fn each_keyterm_is_a_separate_query_parameter_with_spaces_encoded() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        dictate(
            &provider(&url),
            &context(&["Kubernetes", "customer service", "Ada Lovelace"]),
            1,
        )
        .await
        .expect("transcribe");

        let query = server.query();
        assert_eq!(
            query.matches("keyterm=").count(),
            3,
            "one keyterm parameter per term: {query}"
        );
        assert!(query.contains("keyterm=Kubernetes"), "{query}");
        // `form_urlencoded` writes spaces as `+`, which Deepgram documents as
        // an accepted phrase separator alongside %20.
        assert!(query.contains("keyterm=customer+service"), "{query}");
        assert!(query.contains("keyterm=Ada+Lovelace"), "{query}");
    }

    #[tokio::test]
    async fn focused_app_accessibility_and_ocr_become_contextual_keyterms() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        let mut ctx = context(&["Kubernetes"]);
        ctx.app.name = "Visual Studio Code".into();
        ctx.ax_context = vec![
            "Editing Project Aurora in src/api_client.rs".into(),
            "ordinary lowercase prose is not terminology".into(),
        ];
        ctx.ocr_context = vec!["Deploy AcmeCloud v2".into()];

        dictate(&provider(&url), &ctx, 1).await.expect("transcribe");

        let query = server.query();
        for expected in [
            "keyterm=Kubernetes",
            "keyterm=Visual+Studio+Code",
            "keyterm=Editing+Project+Aurora",
            "keyterm=src%2Fapi_client.rs",
            "keyterm=Deploy+AcmeCloud+v2",
        ] {
            assert!(query.contains(expected), "{query} is missing {expected}");
        }
        assert!(
            !query.contains("ordinary"),
            "generic OCR prose leaked: {query}"
        );
    }

    #[tokio::test]
    async fn dictation_commands_are_sent_only_when_enabled_for_english() {
        for (enabled, language, expected) in [
            (true, LanguageMode::Explicit("en-GB".into()), true),
            (false, LanguageMode::Explicit("en".into()), false),
            (true, LanguageMode::Explicit("fr".into()), false),
            (true, LanguageMode::Explicit(MULTILINGUAL.into()), false),
            (true, LanguageMode::Detect, false),
        ] {
            let (url, server) = start_server(vec![finalized("hi")]).await;
            let mut p = provider(&url);
            p.config.dictation = enabled;
            p.config.language = language;
            dictate(&p, &context(&[]), 1).await.expect("transcribe");
            assert_eq!(
                server.query().contains("dictation=true"),
                expected,
                "{}",
                server.query()
            );
        }
    }

    #[tokio::test]
    async fn malformed_dictionary_entries_never_reach_the_wire() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        // Deepgram answers 200 for every one of these and boosts nothing,
        // so they must be filtered client-side.
        dictate(
            &provider(&url),
            &context(&["Kubernetes", "a,b", "a;b", "term:0.15", "  "]),
            1,
        )
        .await
        .expect("transcribe");

        let query = server.query();
        assert_eq!(query.matches("keyterm=").count(), 1, "{query}");
        assert!(query.contains("keyterm=Kubernetes"), "{query}");
    }

    #[test]
    fn the_keyterm_list_is_capped_at_fifty_phrases_well_under_the_token_ceiling() {
        // Sixty single-token phrases: under the 500-token API limit, over the
        // phrase cap. Truncation must keep the highest-priority terms.
        let vocabulary: Vec<String> = (0..60).map(|i| format!("term{i}")).collect();
        let ctx = DictationContext {
            dictionary: DictionaryContext {
                vocabulary,
                ..Default::default()
            },
            ..Default::default()
        };
        let selected = keyterms(&ctx);

        assert_eq!(selected.len(), KEYTERM_MAX_PHRASES);
        assert_eq!(selected.first().map(String::as_str), Some("term0"));
        assert_eq!(selected.last().map(String::as_str), Some("term49"));
    }

    #[test]
    fn the_token_ceiling_still_binds_when_the_phrases_are_long() {
        // Forty phrases of twenty tokens each is 800 tokens, so Deepgram's
        // hard 500-token limit bites at twenty-five phrases — long before the
        // fifty-phrase cap. Both limits are real and neither subsumes the
        // other. (Twenty `word`s is 99 characters, just inside the 100-char
        // ceiling `is_valid_keyterm` enforces.)
        let phrase = std::iter::repeat_n("word", 20)
            .collect::<Vec<_>>()
            .join(" ");
        let vocabulary: Vec<String> = std::iter::repeat_n(phrase, 40).collect();
        let ctx = DictationContext {
            dictionary: DictionaryContext {
                vocabulary,
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(keyterms(&ctx).len(), 25);
    }

    #[tokio::test]
    async fn keyterms_are_omitted_entirely_when_boosting_is_off() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        let mut p = provider(&url);
        p.config.keyterm_boost = false;

        dictate(&p, &context(&["Kubernetes"]), 1)
            .await
            .expect("transcribe");
        assert!(!server.query().contains("keyterm"));
    }

    #[tokio::test]
    async fn keyterms_are_still_sent_on_a_model_that_ignores_them() {
        // The warning is a diagnostic, not a behaviour change: suppressing the
        // keyterms would be a second silent change on top of the first.
        let (url, server) = start_server(vec![finalized("hi")]).await;
        let mut p = provider(&url);
        p.config.model = "nova-2".to_string();

        dictate(&p, &context(&["Kubernetes"]), 1)
            .await
            .expect("transcribe");

        let query = server.query();
        assert!(query.contains("model=nova-2"), "{query}");
        assert!(query.contains("keyterm=Kubernetes"), "{query}");
    }

    /// Language mode must not disable keyterms: multilingual Nova-3 supports
    /// them, while models outside the Nova-3 family do not.
    #[test]
    fn only_the_model_family_can_make_keyterm_boosting_ineffective() {
        use KeytermRisk::{ModelLacksKeyterm, None as NoRisk};

        for (boost, model, expected) in [
            (true, "nova-3", NoRisk),
            (true, "nova-3-medical", NoRisk),
            (true, "nova-2", ModelLacksKeyterm),
            (false, "nova-3", NoRisk),
            (false, "nova-3-medical", NoRisk),
            (false, "nova-2", NoRisk),
        ] {
            let config = DeepgramConfig {
                keyterm_boost: boost,
                model: model.to_string(),
                ..Default::default()
            };
            assert_eq!(
                config.keyterm_risk(),
                expected,
                "boost={boost} model={model}"
            );
        }
    }

    #[test]
    fn the_nova_3_family_test_is_a_prefix_test_so_variants_keep_their_keyterms() {
        // An exact `== "nova-3"` comparison is the bug: it strips keyterm
        // support from models that genuinely have it.
        assert!(is_nova3_family("nova-3"));
        assert!(is_nova3_family("nova-3-general"));
        assert!(is_nova3_family("nova-3-medical"));
        assert!(is_nova3_family("  nova-3-medical  "));

        assert!(!is_nova3_family("nova-2"));
        assert!(!is_nova3_family("nova-2-general"));
        assert!(!is_nova3_family("enhanced"));
        assert!(!is_nova3_family("whisper-large"));
        assert!(!is_nova3_family(""));
        // The trailing hyphen keeps an unrelated future id out of the family.
        assert!(!is_nova3_family("nova-30"));
    }

    // -- Streaming lifecycle -----------------------------------------------

    #[tokio::test]
    async fn pcm_packets_arrive_as_binary_frames_in_order() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        dictate(&provider(&url), &context(&[]), 3)
            .await
            .expect("transcribe");

        let frames = server.binary_frames();
        assert_eq!(frames.len(), 3, "one binary frame per packet");
        let expected: Vec<u8> = packet().iter().flat_map(|s| s.to_le_bytes()).collect();
        for frame in &frames {
            assert_eq!(frame.len(), wl_core::consts::CHUNK_BYTES);
            assert_eq!(
                frame, &expected,
                "samples must survive as little-endian s16"
            );
        }
    }

    #[tokio::test]
    async fn the_session_drains_with_finalize_and_then_closes_the_stream() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        dictate(&provider(&url), &context(&[]), 1)
            .await
            .expect("transcribe");

        // CloseStream is sent after the outcome is delivered, so give the
        // socket task its 200 ms grace before reading the recording.
        tokio::time::sleep(CLOSE_GRACE * 3).await;
        assert_eq!(
            server.text_frames(),
            vec![FINALIZE_FRAME.to_string(), CLOSE_FRAME.to_string()],
            "Finalize must precede CloseStream, and nothing else may be sent"
        );
    }

    #[tokio::test]
    async fn the_transcript_is_read_from_the_first_alternative_of_the_channel() {
        let (url, _) = start_server(vec![json!({
            "type": "Results",
            "is_final": true,
            "from_finalize": true,
            "channel": { "alternatives": [
                { "transcript": "the right one" },
                { "transcript": "the wrong one" },
            ] },
        })])
        .await;

        let ctx = context(&[]);
        let result = dictate(&provider(&url), &ctx, 5).await.expect("transcribe");
        assert_eq!(result.asr_text.as_deref(), Some("the right one"));
        assert_eq!(result.id, "TID-1");
        // Five 40 ms packets. Streaming carries no session duration, so the
        // packet count is the only measure available.
        assert!((result.duration_secs - 0.2).abs() < 1e-9, "{result:?}");
    }

    #[tokio::test]
    async fn consecutive_final_segments_are_joined_with_a_space() {
        let (url, _) = start_server_with(
            vec![finalized("world")],
            vec![results("hello", true, false)],
            None,
        )
        .await;

        let ctx = context(&[]);
        let result = dictate(&provider(&url), &ctx, 1).await.expect("transcribe");
        assert_eq!(result.asr_text.as_deref(), Some("hello world"));
    }

    #[tokio::test]
    async fn interim_frames_never_become_segments() {
        // With `interim_results=false` these should not arrive, but counting
        // one would duplicate the text of the final that follows it.
        let (url, _) = start_server_with(
            vec![finalized("hello world")],
            vec![
                results("hello", false, false),
                results("hello wor", false, false),
            ],
            None,
        )
        .await;

        let ctx = context(&[]);
        let result = dictate(&provider(&url), &ctx, 1).await.expect("transcribe");
        assert_eq!(result.asr_text.as_deref(), Some("hello world"));
    }

    #[tokio::test]
    async fn frames_that_are_not_results_are_ignored() {
        let (url, _) = start_server_with(
            vec![finalized("hi")],
            vec![
                json!({ "type": "Metadata", "request_id": "r-1", "model_uuid": "m" }),
                json!({ "type": "SpeechStarted", "timestamp": 0.1 }),
                json!({ "type": "UtteranceEnd", "last_word_end": 1.0 }),
            ],
            None,
        )
        .await;

        let ctx = context(&[]);
        let result = dictate(&provider(&url), &ctx, 1).await.expect("transcribe");
        assert_eq!(result.asr_text.as_deref(), Some("hi"));
    }

    #[tokio::test]
    async fn an_empty_transcript_is_reported_as_no_transcription() {
        let (url, _) = start_server(vec![finalized("   ")]).await;
        let ctx = context(&[]);

        let err = dictate(&provider(&url), &ctx, 1)
            .await
            .expect_err("silence must not be a successful empty insertion");
        assert_eq!(err, ProviderError::EmptyResult);
    }

    #[tokio::test]
    async fn an_empty_final_frame_from_finalize_completes_without_waiting_out_the_deadline() {
        // After Finalize on silence, an empty `is_final` frame is the only
        // answer Deepgram sends. Treating it as "nothing yet" would stall the
        // dictation for the whole finalize timeout.
        let (url, _) = start_server(vec![json!({
            "type": "Results",
            "is_final": true,
            "from_finalize": true,
            "channel": { "alternatives": [] },
        })])
        .await;

        let ctx = context(&[]);
        // A five-second deadline against a sub-second assertion: the point is
        // that the empty frame short-circuits the wait, not how fast loopback
        // is on a loaded machine.
        let started = std::time::Instant::now();
        let err = dictate(
            &provider_with_finalize(&url, Duration::from_secs(5)),
            &ctx,
            1,
        )
        .await
        .expect_err("empty");

        assert_eq!(err, ProviderError::EmptyResult);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "completed only after the finalize deadline: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_server_that_never_answers_finalize_still_yields_what_it_already_sent() {
        let (url, _) = start_server_with(
            Vec::new(),
            vec![results("already transcribed", true, false)],
            None,
        )
        .await;

        let ctx = context(&[]);
        let started = std::time::Instant::now();
        let result = dictate(&provider(&url), &ctx, 1).await.expect("transcribe");

        assert_eq!(result.asr_text.as_deref(), Some("already transcribed"));
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "the deadline must actually be waited out"
        );
    }

    #[tokio::test]
    async fn a_silent_stream_is_kept_alive_so_deepgram_does_not_hang_up() {
        // Deepgram closes an idle socket after 10 s. Push-to-talk holds are
        // routinely silent for longer than that — the user presses, thinks,
        // then speaks — and without the timer the connection is gone before
        // the first word. Real clock with an injected 40 ms cadence: a paused
        // clock auto-advances in timer-wheel steps and would make the observed
        // count meaningless.
        let (url, server) = start_server(vec![finalized("hi")]).await;
        let mut p = provider(&url);
        p.timings.keepalive_interval = Duration::from_millis(40);
        p.timings.keepalive_idle = Duration::from_millis(30);

        let ctx = context(&[]);
        let session = p.start(&ctx).await.expect("start");
        // No packets at all, which is precisely the case the timer exists for.
        tokio::time::sleep(Duration::from_millis(250)).await;
        session.finish(&ctx).await.expect("transcribe");

        let sent = server.text_frames();
        let keepalives = sent.iter().filter(|f| f.contains("KeepAlive")).count();
        assert!(
            keepalives >= 2,
            "a silent stream must be kept alive repeatedly, not once: {sent:?}"
        );
        assert!(sent
            .iter()
            .all(|f| f == KEEPALIVE_FRAME || f == FINALIZE_FRAME || f == CLOSE_FRAME));
    }

    #[tokio::test]
    async fn an_active_stream_sends_no_keep_alives_because_the_audio_is_the_heartbeat() {
        // The skip window is not an optimization detail: without it every
        // dictation interleaves control frames into the audio stream for no
        // reason.
        let (url, server) = start_server(vec![finalized("hi")]).await;
        let mut p = provider(&url);
        p.timings.keepalive_interval = Duration::from_millis(40);
        p.timings.keepalive_idle = Duration::from_millis(200);

        let ctx = context(&[]);
        let session = p.start(&ctx).await.expect("start");
        for _ in 0..25 {
            session.feed(&packet());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        session.finish(&ctx).await.expect("transcribe");

        let sent = server.text_frames();
        assert!(
            !sent.iter().any(|f| f.contains("KeepAlive")),
            "audio already keeps the stream alive: {sent:?}"
        );
        assert_eq!(server.binary_frames().len(), 25);
    }

    #[tokio::test]
    async fn the_detected_language_is_read_from_the_streaming_shape() {
        // Under `language=multi` Deepgram reports languages on the
        // *alternative*, not on the channel — the field Swift reads and never
        // finds.
        let (url, _) = start_server(vec![json!({
            "type": "Results",
            "is_final": true,
            "from_finalize": true,
            "channel": { "alternatives": [
                { "transcript": "no recuerdo mi bank password", "languages": ["es", "en"] },
            ] },
        })])
        .await;

        let ctx = context(&[]);
        let result = dictate(&provider(&url), &ctx, 1).await.expect("transcribe");
        assert_eq!(
            result.asr_text.as_deref(),
            Some("no recuerdo mi bank password")
        );

        let mut outcome = StreamOutcome::default();
        absorb(
            &mut outcome,
            &json!({
                "type": "Results",
                "is_final": true,
                "channel": { "alternatives": [{ "transcript": "hola", "languages": ["es"] }] },
            })
            .to_string(),
        );
        assert_eq!(outcome.detected_language.as_deref(), Some("es"));
    }

    #[test]
    fn the_pre_recorded_detected_language_field_is_still_honoured() {
        // Streaming detection is not shipped; when it is, `detected_language`
        // is the field it will use, and dropping it would lose the answer.
        let mut outcome = StreamOutcome::default();
        assert!(absorb(
            &mut outcome,
            &json!({
                "type": "Results",
                "is_final": true,
                "from_finalize": true,
                "channel": {
                    "alternatives": [{ "transcript": "bonjour" }],
                    "detected_language": "fr",
                },
            })
            .to_string(),
        ));
        assert_eq!(outcome.segments, vec!["bonjour".to_string()]);
        assert_eq!(outcome.detected_language.as_deref(), Some("fr"));
    }

    #[tokio::test]
    async fn cancelling_produces_no_transcript_and_closes_the_socket() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        let ctx = context(&[]);

        let session = provider(&url).start(&ctx).await.expect("start");
        session.feed(&packet());
        session.cancel();

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            server.text_frames().is_empty(),
            "cancel must not ask the server to finalize"
        );
    }

    // -- Errors ------------------------------------------------------------

    #[tokio::test]
    async fn a_rejected_key_on_the_upgrade_is_an_actionable_auth_failure() {
        let (url, _) = start_server_with(Vec::new(), Vec::new(), Some(401)).await;
        let err = provider(&url)
            .start(&context(&[]))
            .await
            .err()
            .expect("401 must fail");

        assert!(!err.is_retryable(), "a rejected key stays rejected");
        assert!(
            err.user_message().contains("console.deepgram.com"),
            "the message must say where to get a new key: {}",
            err.user_message()
        );
    }

    #[tokio::test]
    async fn a_bad_request_on_the_upgrade_blames_the_model_not_the_credentials() {
        let (url, _) = start_server_with(Vec::new(), Vec::new(), Some(400)).await;
        let err = provider(&url)
            .start(&context(&[]))
            .await
            .err()
            .expect("400 must fail");

        assert_eq!(
            err,
            ProviderError::ServerError(
                "Deepgram rejected the request (HTTP 400) \u{2014} check the configured model \
                 and language"
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn a_rate_limited_upgrade_is_retryable_not_a_quota_failure() {
        let (url, _) = start_server_with(Vec::new(), Vec::new(), Some(429)).await;
        let err = provider(&url)
            .start(&context(&[]))
            .await
            .err()
            .expect("429 must fail");

        assert_eq!(
            err,
            ProviderError::RateLimited {
                provider: "Deepgram"
            }
        );
        assert!(err.is_retryable(), "a rate limit clears on its own");
    }

    #[tokio::test]
    async fn an_unreachable_endpoint_is_a_connection_failure() {
        // Port 0 never accepts, and no socket means no transcript.
        let p = DeepgramProvider::new(
            DeepgramConfig::default()
                .with_api_key("test-key")
                .with_stream_url("ws://127.0.0.1:1"),
        )
        .with_credential_store(empty_store());

        assert_eq!(
            p.start(&context(&[])).await.err(),
            Some(ProviderError::ConnectionFailed)
        );
    }

    #[tokio::test]
    async fn a_missing_api_key_fails_without_opening_a_socket() {
        let (url, server) = start_server(vec![finalized("hi")]).await;
        let p = DeepgramProvider::new(DeepgramConfig::default().with_stream_url(url))
            .with_credential_store(empty_store());

        assert_eq!(
            p.start(&context(&[])).await.err(),
            Some(ProviderError::NotConfigured {
                provider: "Deepgram"
            })
        );
        assert!(
            server.query.lock().expect("query").is_none(),
            "an unconfigured provider must not touch the network"
        );
    }

    // -- Post-processing ---------------------------------------------------

    #[tokio::test]
    async fn post_processing_applies_the_dictionary_and_leaves_the_raw_text_intact() {
        let (url, _) = start_server(vec![finalized("deploy to cube er netties")]).await;

        let mut ctx = context(&[]);
        ctx.dictionary.replacements =
            [("cube er netties".to_string(), "Kubernetes".to_string())].into();
        ctx.dictionary.snippets = [("deploy".to_string(), "ship".to_string())].into();

        let result = dictate(&provider(&url), &ctx, 1).await.expect("transcribe");
        assert_eq!(
            result.asr_text.as_deref(),
            Some("deploy to cube er netties")
        );
        // Replacements before snippets, then the tidy-up.
        assert_eq!(
            result.formatted_text.as_deref(),
            Some("Ship to Kubernetes.")
        );
        assert_eq!(result.display_text(), "Ship to Kubernetes.");
    }

    #[tokio::test]
    async fn disabling_post_processing_publishes_the_raw_transcript() {
        let (url, _) = start_server(vec![finalized("deploy to cube er netties")]).await;
        let mut p = provider(&url);
        p.config.post_process = PostProcessOptions::DISABLED;

        let mut ctx = context(&[]);
        ctx.dictionary.replacements =
            [("cube er netties".to_string(), "Kubernetes".to_string())].into();

        let result = dictate(&p, &ctx, 1).await.expect("transcribe");
        assert_eq!(result.formatted_text, None);
        assert_eq!(result.display_text(), "deploy to cube er netties");
    }

    #[tokio::test]
    async fn the_word_count_describes_the_text_that_will_be_inserted() {
        let (url, _) = start_server(vec![finalized("expand sig")]).await;
        let mut ctx = context(&[]);
        ctx.dictionary.snippets = [("sig".to_string(), "Best regards Ada".to_string())].into();

        let result = dictate(&provider(&url), &ctx, 1).await.expect("transcribe");
        assert_eq!(
            result.formatted_text.as_deref(),
            Some("Expand Best regards Ada.")
        );
        assert_eq!(result.num_words, 4);
    }

    // -- Language ----------------------------------------------------------

    #[tokio::test]
    async fn auto_detect_uses_the_documented_multilingual_streaming_contract() {
        // Deepgram documents that language detection is not supported for
        // streaming and recommends the multilingual model instead.
        let (url, server) = start_server(vec![finalized("bonjour")]).await;
        let mut p = provider(&url);
        p.config.language = LanguageMode::Detect;

        dictate(&p, &context(&[]), 1).await.expect("transcribe");

        let query = server.query();
        assert!(
            query.contains("language=multi"),
            "auto-detect must still transcribe non-English speech: {query}"
        );
        assert!(
            !query.contains("detect_language"),
            "unsupported streaming detection makes Deepgram reject the upgrade: {query}"
        );
    }

    #[tokio::test]
    async fn an_explicit_language_is_the_only_language_parameter_sent() {
        let (url, server) = start_server(vec![finalized("hallo")]).await;
        let mut p = provider(&url);
        p.config.language = LanguageMode::Explicit("de-CH".into());

        dictate(&p, &context(&[]), 1).await.expect("transcribe");

        let query = server.query();
        assert!(query.contains("language=de-CH"), "{query}");
        assert!(!query.contains("detect_language"), "{query}");
    }

    #[test]
    fn deepgrams_own_picker_uses_the_swift_sentinels() {
        assert_eq!(language_mode_for(AUTO_DETECT), LanguageMode::Detect);
        assert_eq!(
            language_mode_for(MULTI_SELECT),
            LanguageMode::Explicit(MULTILINGUAL.into())
        );
        assert_eq!(
            language_mode_for(""),
            LanguageMode::Explicit(DEFAULT_LANGUAGE.into()),
            "an unset picker means Deepgram's own default, not a missing tag"
        );
        assert_eq!(
            language_mode_for("nl-BE"),
            LanguageMode::Explicit("nl-BE".into()),
            "the 35 Deepgram codes are already BCP-47 and pass through"
        );
    }

    #[test]
    fn the_shared_pickers_auto_sentinel_still_selects_detection() {
        // `ui/src/settings/languages.ts` writes `auto` into the shared
        // `languages` list, and an upgraded profile has nothing else. Reading
        // it as a language tag would send `language=auto`, which Deepgram
        // accepts with HTTP 200 and ignores.
        assert_eq!(
            language_mode(&[SHARED_AUTO_DETECT.to_string()]),
            LanguageMode::Detect
        );
        assert_eq!(language_mode_for(SHARED_AUTO_DETECT), LanguageMode::Detect);
    }

    #[test]
    fn one_configured_language_is_sent_verbatim_and_several_become_multi() {
        use LanguageMode::Explicit;
        assert_eq!(language_mode(&[]), Explicit("en".into()));
        assert_eq!(
            language_mode(&["en-GB".to_string()]),
            Explicit("en-GB".into())
        );
        assert_eq!(language_mode(&["  ".to_string()]), Explicit("en".into()));
        assert_eq!(
            language_mode(&["de".to_string(), "fr".to_string()]),
            Explicit("multi".into())
        );
        assert_eq!(
            language_mode(&["de".to_string(), "  ".to_string()]),
            Explicit("de".into()),
            "blank entries are not a second language"
        );
    }

    #[test]
    fn the_pickers_non_standard_codes_are_translated_to_deepgram_tags() {
        use LanguageMode::Explicit;
        // Passing the UI's legacy short codes through verbatim can quietly
        // select the wrong language.
        assert_eq!(
            language_mode(&["engb".to_string()]),
            Explicit("en-GB".into())
        );
        assert_eq!(
            language_mode(&["dech".to_string()]),
            Explicit("de-CH".into())
        );
        assert_eq!(
            language_mode(&["zhcn".to_string()]),
            Explicit("zh-Hans".into())
        );
        assert_eq!(
            language_mode(&["yue".to_string()]),
            Explicit("zh-HK".into())
        );
    }

    #[test]
    fn the_pickers_bare_zh_means_traditional_chinese_not_deepgrams_simplified() {
        // The worst case in the table: `zh` is a tag Deepgram DOES accept, but
        // it means Simplified there and Traditional in the picker, so leaving
        // it alone transcribes into the wrong script with no error anywhere.
        assert_eq!(
            language_mode(&["zh".to_string()]),
            LanguageMode::Explicit("zh-Hant".into())
        );
    }

    #[test]
    fn hinglish_maps_to_code_switching_because_nova_3_has_no_hinglish_tag() {
        // `hi-Latn` exists only on the legacy nova/base models. Hinglish is
        // Hindi/English code-switching and `multi` spans exactly that pair.
        assert_eq!(
            language_mode(&["hien".to_string()]),
            LanguageMode::Explicit(MULTILINGUAL.into())
        );
    }

    #[test]
    fn already_valid_tags_and_unsupported_ones_pass_through_untouched() {
        // Plain ISO-639-1 codes are already valid BCP-47.
        for code in ["fr", "de", "ja", "pt", "es"] {
            assert_eq!(deepgram_language_tag(code), code);
        }
        // Regioned tags a user may have typed by hand are left alone.
        assert_eq!(deepgram_language_tag("pt-BR"), "pt-BR");
        // Languages nova-3 does not support are NOT silently substituted: a
        // rejected request the user can act on beats a wrong transcript.
        for code in ["cy", "la", "mi", "haw"] {
            assert_eq!(deepgram_language_tag(code), code);
        }
    }

    #[test]
    fn detection_wins_regardless_of_where_it_appears_in_the_shared_list() {
        // The settings model makes Auto-detect exclusive, so a mixed list
        // should not occur — but the mapping must not be order-dependent, or a
        // stale settings.json would silently send `language=fr`.
        assert_eq!(
            language_mode(&["fr".to_string(), "auto".to_string()]),
            LanguageMode::Detect
        );
        assert_eq!(
            language_mode(&["auto".to_string(), "fr".to_string()]),
            LanguageMode::Detect
        );
        assert_eq!(language_mode(&[" AUTO ".to_string()]), LanguageMode::Detect);
    }

    // -- Settings ----------------------------------------------------------

    #[test]
    fn deepgrams_own_language_field_wins_over_the_shared_list() {
        let settings = Settings {
            deepgram_language: MULTI_SELECT.into(),
            ..Default::default()
        };
        assert_eq!(
            settings_language(&settings),
            LanguageMode::Explicit(MULTILINGUAL.into())
        );
    }

    /// A fresh profile detects the language instead of assuming English.
    ///
    /// `DEFAULT_LANGUAGE` still backs an *empty* setting — that is Deepgram's
    /// own fallback for a blank field, and is asserted separately. This covers
    /// the shipped default, which is the auto-detect sentinel.
    #[test]
    fn a_fresh_profile_detects_the_language() {
        assert_eq!(settings_language(&Settings::default()), LanguageMode::Detect);
    }

    #[test]
    fn an_empty_language_field_still_falls_back_to_deepgrams_own_default() {
        let settings = Settings {
            deepgram_language: String::new(),
            ..Default::default()
        };
        assert_eq!(
            settings_language(&settings),
            LanguageMode::Explicit(DEFAULT_LANGUAGE.into())
        );
    }

    #[test]
    fn building_the_config_reads_settings_and_never_a_credential() {
        let settings = Settings {
            deepgram_language: "fr".into(),
            deepgram_model: "nova-3-medical".into(),
            deepgram_keyterm_boost: false,
            ..Default::default()
        };

        let config = DeepgramConfig::from_settings(&settings);
        // Rebuilding settings must never read or write credentials.
        assert_eq!(config.api_key, None);
        assert_eq!(config.language, LanguageMode::Explicit("fr".into()));
        assert_eq!(config.model, "nova-3-medical");
        assert!(!config.keyterm_boost);
        assert!(config.post_process.enabled);
        assert_eq!(config.stream_url, DEFAULT_STREAM_URL);
    }

    #[test]
    fn an_empty_configured_model_falls_back_to_the_keyterm_capable_default() {
        let settings = Settings {
            deepgram_model: String::new(),
            ..Default::default()
        };
        let config = DeepgramConfig::from_settings(&settings);
        assert_eq!(config.model, DEFAULT_MODEL);
    }

    #[test]
    fn the_environment_overrides_the_stored_key_and_blank_values_do_not_count() {
        // Precedence, in order, with blanks skipped rather than winning.
        assert_eq!(
            choose_key(Some("explicit"), Some("env"), Some("stored")).as_deref(),
            Some("explicit")
        );
        assert_eq!(
            choose_key(None, Some("env"), Some("stored")).as_deref(),
            Some("env")
        );
        assert_eq!(
            choose_key(None, None, Some("stored")).as_deref(),
            Some("stored")
        );
        assert_eq!(choose_key(None, None, None), None);
        // A blank saved key is the shape a cleared secure field leaves behind.
        assert_eq!(choose_key(None, None, Some("   ")), None);
        assert_eq!(
            choose_key(Some("  "), None, Some("stored")).as_deref(),
            Some("stored")
        );
        // Whitespace around a real key is a paste artefact, not part of it.
        assert_eq!(
            choose_key(None, None, Some(" dg-secret ")).as_deref(),
            Some("dg-secret")
        );
    }

    #[tokio::test]
    async fn the_stored_key_is_used_when_no_override_is_configured() {
        let dir = std::env::temp_dir().join(format!("wl-dg-{}", uuid::Uuid::new_v4()));
        let store = CredentialStore::file_backed(dir.join("credentials.json"));
        store.set(DEEPGRAM_API_KEY, "dg-secret").expect("store key");

        let (url, server) = start_server(vec![finalized("hi")]).await;
        let mut p = DeepgramProvider::new(DeepgramConfig::default().with_stream_url(url))
            .with_credential_store(store);
        assert!(
            p.is_ready(),
            "a key saved in the credential store must pass the hotkey readiness gate"
        );
        p.timings.finalize = Duration::from_millis(300);

        let ctx = context(&[]);
        let sent = dictate(&p, &ctx, 1).await;
        std::fs::remove_dir_all(&dir).ok();

        sent.expect("transcribe");
        assert_eq!(
            server.authorization.lock().expect("auth").as_deref(),
            Some("Token dg-secret")
        );
    }

    // -- Health -------------------------------------------------------------

    fn health_provider(server: &MockServer) -> DeepgramProvider {
        DeepgramProvider::new(
            DeepgramConfig::default()
                .with_api_key("test-key")
                .with_base_url(server.uri()),
        )
        .with_credential_store(empty_store())
    }

    async fn auth_token_server(response: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/auth/token"))
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn health_validates_the_key_against_the_auth_token_endpoint() {
        let server = auth_token_server(ResponseTemplate::new(200).set_body_json(json!({
            "api_key_id": "k-1", "scopes": ["member"]
        })))
        .await;

        health_provider(&server).health().await.expect("healthy");

        let requests = server.received_requests().await.expect("recording enabled");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].headers.get("authorization").expect("auth"),
            "Token test-key"
        );
    }

    #[tokio::test]
    async fn health_maps_a_rejected_key_to_an_actionable_auth_failure() {
        let server =
            auth_token_server(ResponseTemplate::new(401).set_body_string("invalid credentials"))
                .await;
        let err = health_provider(&server)
            .health()
            .await
            .expect_err("401 must fail");

        assert!(!err.is_retryable());
        assert!(err.user_message().contains("console.deepgram.com"));
    }

    #[tokio::test]
    async fn health_maps_a_rate_limit_to_a_retryable_failure() {
        let server =
            auth_token_server(ResponseTemplate::new(429).set_body_string("too many")).await;
        assert_eq!(
            health_provider(&server).health().await,
            Err(ProviderError::RateLimited {
                provider: "Deepgram"
            })
        );
    }

    #[tokio::test]
    async fn health_maps_an_exhausted_balance_to_a_quota_failure() {
        let server =
            auth_token_server(ResponseTemplate::new(402).set_body_string("out of credit")).await;
        assert_eq!(
            health_provider(&server).health().await,
            Err(ProviderError::QuotaExceeded {
                provider: "Deepgram"
            })
        );
    }

    #[tokio::test]
    async fn health_without_a_key_reports_the_provider_as_unconfigured() {
        let server = auth_token_server(ResponseTemplate::new(200)).await;
        let p = DeepgramProvider::new(DeepgramConfig::default().with_base_url(server.uri()))
            .with_credential_store(empty_store());

        assert_eq!(
            p.health().await,
            Err(ProviderError::NotConfigured {
                provider: "Deepgram"
            })
        );
        assert!(server
            .received_requests()
            .await
            .expect("recording enabled")
            .is_empty());
    }
}
