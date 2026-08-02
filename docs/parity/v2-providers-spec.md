# Wispr Lightning v2 — Multi-Vendor Transcription Architecture Spec

Source of truth: `origin/feature/backlog-sweep` (head `8a81d74`). Every literal below was read out of that tree; nothing is paraphrased from memory.

Files covered:

| File | Lines | Role |
|---|---|---|
| `Sources/WisprLightning/Services/DictationProvider.swift` | 101 | protocol, `DictationVendor`, `DictationContext` |
| `Services/Providers/WisprFlowProvider.swift` | 615 | Flow WSS, buffered upload |
| `Services/Providers/OpenRouterProvider.swift` | 242 | OpenRouter chat-completions, inline WAV |
| `Services/Providers/ClaudeVoiceProvider.swift` | 265 | Claude Code STT, live streaming |
| `Services/Providers/ClaudeVoice/VoiceStream.swift` | 367 | Claude Voice WS client |
| `Services/Providers/ClaudeVoice/ClaudeCodeKeychain.swift` | 190 | `Claude Code-credentials` reader + mirror |
| `Services/Providers/ClaudeVoice/KeyTerms.swift` | 68 | NLTagger vocabulary boost |
| `Services/Providers/DeepgramProvider.swift` | 524 | Deepgram `/v1/listen` **streaming** WSS |
| `Services/AudioEncoding.swift` | 54 | WAV / base64 |
| `Services/OpenRouterModels.swift` | 94 | `/api/v1/models` picker feed |
| `Services/SecretsStore.swift` | 146 | file-backed secrets |
| `Services/KeychainStore.swift` | 140 | legacy Keychain (OpenRouter only) |
| `Services/SafeCompletion.swift` | 51 | fire-once completion gate |
| `Models/Settings.swift` | — | `FallbackStep`, `AppSettings` vendor fields |
| `Models/Session.swift` | — | `canUsePolish(activeVendor:)` |
| `Models/TranscriptEntry.swift` | — | `TranscriptResult`, `TranscriptionError` |
| `App/AppDelegate.swift` | — | chain orchestration, watchdog, provider factory |

Backlog provenance: **B-007** (protocol + polish gate, commit `0808f00`), **B-008** (OpenRouter, `2b87b70`), **B-009** (Claude Voice, `71e32d4`, OCR-keyterms follow-up `f272742`), **B-012** (fallback chain, `25b3315` — B-012 has no `## B-012` entry in `BACKLOG.md`; the commit is the only record), Deepgram head commit `8a81d74`.

---

## 0. Global constants used by every provider (`Services/Constants.swift`)

```
supabaseURL   = "https://dodjkfqhwrzqjwkfnthl.supabase.co"
wsURL         = "wss://api.wisprflow.ai/llm/ws"
apiURL        = "https://api.wisprflow.ai"
sampleRate    = 16000
channels      = 1
chunkDurationMs = 40
chunkSamples  = 640            // sampleRate * chunkDurationMs / 1000
clientVersion = "1.4.549"
maxRecordingSeconds = 600
warningSeconds = 540
finalWarningSeconds = 570
```

One **packet** = 640 samples of 16 kHz mono signed-16-bit little-endian PCM = **1280 bytes** = **40 ms**. Every provider receives packets in exactly this shape via `feed(packet:)`. Duration is always recomputed as `packetCount * 40 / 1000` seconds, never measured with a clock.

### What the Rust port must change
Nothing here — `wl_core::consts` already matches. But note `wsURL` is the **full path** `wss://api.wisprflow.ai/llm/ws`, and every vendor's audio unit is the 1280-byte packet, including the two providers that stream it raw over a WebSocket binary frame.

---

## 1. The `DictationProvider` protocol

### 1.1 Verbatim declaration

```swift
protocol DictationProvider: AnyObject {
    var dictionaryStore: DictionaryStore? { get set }

    func prewarmConnection()
    func cancelPrewarmedConnection()
    func clearEncodingCache()

    func start()
    func feed(packet: Data)
    func stop(context: DictationContext,
              completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void)
    func cancel()
}

extension DictationProvider {
    func prewarmConnection() {}
    func cancelPrewarmedConnection() {}
    func clearEncodingCache() {}
}
```

The doc comment states the lifecycle exactly:

```
prewarmConnection()        // optional, ahead of recording
start()                    // begin a new session
feed(packet:) * N          // PCM packets as they're captured
stop(context:, completion:) // finalize, deliver result
cancel()                   // abort without delivering
```

and: *"`feed(packet:)` may stream live (Claude Voice) or buffer internally (Wispr Flow / OpenRouter) — that's a provider-level decision."*

### 1.2 Method semantics — exact

| Method | Required? | Semantics |
|---|---|---|
| `dictionaryStore` | required, settable property | Injected by `AppDelegate` immediately after every provider construction (`dictationProvider.dictionaryStore = dictionaryStore`) — at launch, on vendor switch, on manual retry, and on **every** chain hop. Providers read `getVocabularyPhrases()`, `getReplacements()`, `getSnippets()` off it at request-build time, never at init. |
| `prewarmConnection()` | **default no-op** | Start TCP+TLS early. Only `WisprFlowProvider` implements it (opens a WS task and, if `!session.isValid`, kicks a background `session.refresh`). Called by `AppDelegate` (a) at hotkey press, (b) during the 1.5 s auto-retry delay, (c) in `retryTranscription()`. Failure is never surfaced. |
| `cancelPrewarmedConnection()` | **default no-op** | Tear down a prewarmed socket without touching session state. Only Flow implements it. |
| `clearEncodingCache()` | **default no-op** | Drop the per-recording encode cache. Only Flow implements it (`cachedEncoding = nil`); the cache is keyed on `packets.count` so a retry with the identical packet count reuses the ascii85 work. |
| `start()` | required | Begin a session. Buffering providers (Flow, OpenRouter) just clear their packet buffer with `removeAll(keepingCapacity: true)`. Streaming providers (Claude Voice, Deepgram) **open the WebSocket here** and read credentials here. Must be safe to call again after `cancel()`. |
| `feed(packet:)` | required | One 1280-byte packet. Buffering providers append under an `NSLock`. Streaming providers dispatch onto their serial queue, increment `packetCount`, and either send a binary WS frame (if open) or append to a pre-open buffer. Must never block the audio thread. |
| `stop(context:completion:)` | required | Finalize. `completion` fires **exactly once** with `.success(TranscriptResult)` or `.failure(TranscriptionError)`. Providers may fire on any thread; `AppDelegate` hops to main where it needs to. Buffering providers drain their buffer and do the whole network round-trip here. Streaming providers send their terminator frame and wait for the server's final. |
| `cancel()` | required | Abort with no completion call. Clears buffers, closes sockets, clears failure state. `AppDelegate` calls it before every provider rebuild and before every re-prime. |

### 1.3 `DictationContext`

```swift
struct DictationContext {
    let appInfo: [String: String]
    let ocrContext: [String]
    let axContext: [String]

    static let empty = DictationContext(appInfo: [:], ocrContext: [], axContext: [])
}
```

`appInfo` keys actually read by providers: `"type"`, `"name"`, `"bundle_id"`, `"url"`. Only Wispr Flow consumes any of it. Per the doc comment: *"Some providers (Wispr Flow) send this in the auth message at stop-time; streaming providers (Claude Voice) may use the parts they care about (e.g. keyterms) at start-time and ignore the rest."*

This is a real design constraint: **Claude Voice cannot use `context` at all**, because its keyterms are query-string parameters on a URL fixed at connect time — which happens in `start()`, before `stop(context:)` exists. See §7.2 for the one-recording-behind workaround.

### 1.4 `TranscriptResult` and `TranscriptionError`

```swift
struct TranscriptResult {
    let id: String
    let asrText: String?
    let formattedText: String?
    let duration: Double
    let numWords: Int
}

enum TranscriptionError: Error {
    case authFailed(String?)     // nil → generic message
    case connectionFailed
    case serverError(String)
    case timeout
    case emptyResult

    var isRetryable: Bool {
        // true:  connectionFailed, timeout, serverError
        // false: authFailed, emptyResult
    }

    var shouldFallback: Bool {
        // true:  authFailed, connectionFailed, timeout, serverError
        // false: emptyResult
    }

    var userMessage: String {
        // .authFailed(detail) → detail ?? "Authentication failed — please sign in again"
        // .connectionFailed   → "Connection failed — check your network"
        // .serverError(d)     → "Server error: \(d)"
        // .timeout            → "Request timed out — server did not respond"
        // .emptyResult        → "No transcription returned"
    }
}
```

`numWords` is always `text.split(separator: " ").count`. `duration` is always `packetCount * 40 / 1000`.

**`isRetryable` and `shouldFallback` are two different axes.** `isRetryable` drives the 2× in-place auto-retry against the *same* provider; `shouldFallback` drives the jump to the *next* chain step. `authFailed` is deliberately `shouldFallback == true` but `isRetryable == false` — a rejected key never gets retried in place, it advances the chain immediately. `emptyResult` is false on both: the mic caught nothing and no other vendor will do better.

### 1.5 Why the protocol is streaming, not batch

Three independent forces, in order of strength:

1. **Claude Voice is architecturally incapable of batch.** `wss://api.anthropic.com/api/ws/speech_to_text/voice_stream` has no upload endpoint. Audio is sent as binary WS frames while the user is still speaking; the server emits `TranscriptText` interims and `TranscriptEndpoint` finals as it goes. There is no "here is the whole recording, give me the text" call. `stop()` sends `{"type":"CloseStream"}` and collects what has already arrived. A batch trait cannot express this: by the time you have the whole buffer, the session that would have transcribed it was never opened.
   This is stated in B-007's evidence verbatim: *"`wispr-edge/…/DictationProvider.swift` already defines the protocol shape we want, but uses a batch (`packets: [Data]`) signature that doesn't fit Claude Voice's live-streaming model."*
2. **Deepgram (as shipped in `8a81d74`) is also streaming** — `wss://api.deepgram.com/v1/listen`, live PCM frames, `{"type":"Finalize"}` to drain. Two of four vendors are live-streaming, so streaming is the majority shape, not the exception.
3. **Latency for the buffering vendors is still improved.** Flow's `prewarmConnection()` overlaps TCP+TLS with mic startup; without a `start()`/`feed()` split there is no point at which "recording began" is observable to the provider.

The cost of streaming-as-the-default is borne entirely by the buffering providers, and it is trivial: `start()` clears an array, `feed()` appends to it, `stop()` drains it and does what a batch call would have done. `WisprFlowProvider.stop` is literally:

```swift
bufferLock.lock()
let packets = bufferedPackets
bufferedPackets.removeAll(keepingCapacity: false)
bufferLock.unlock()
transcribe(packets: packets, context: context, completion: completion)
```

### 1.6 Re-priming — the mechanism that makes retries work with a streaming trait

A streaming provider consumed its audio as it arrived; a retry has nothing to replay. `AppDelegate` keeps `pendingPackets: [Data]` (the full recording) and re-feeds it. From `attemptTranscription()`:

```swift
// Re-prime the provider's internal buffer whenever we're talking to a
// provider that wasn't fed live during recording — that's the case
// for every retry (manual or auto), every fallback chain step beyond
// the primary, and after dismissRetry+retryTranscription. The initial
// attempt is skipped because audioRecorder.onPacket already fed it.
if currentRetryAttempt > 0 || currentChainIndex > 0 {
    dictationProvider.cancel()
    dictationProvider.start()
    for packet in packets {
        dictationProvider.feed(packet: packet)
    }
}
```

For a streaming provider this means the entire recording is blasted into an open WebSocket as fast as the socket accepts it, then `stop()` finalizes. Deepgram and Claude Voice both tolerate this (neither rate-limits ingest); the server just sees a very fast talker.

### What the Rust port must change
`wl-providers` currently exposes:

```rust
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn prewarm(&self) {}
    async fn health(&self) -> Result<()>;
    async fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptResult>;
    fn reset(&self) {}
}
```

`transcribe(&self, req: &TranscribeRequest)` takes `packets: Vec<Vec<i16>>` up front. This is the batch shape B-007 explicitly rejected, and it **cannot host Claude Voice at all**. Required reshaping:

- Split into a session type: `fn start(&self) -> Result<Box<dyn DictationSession>>`, with `DictationSession { fn feed(&self, packet: &[i16]); async fn stop(self: Box<Self>, ctx: &DictationContext) -> Result<TranscriptResult>; fn cancel(self: Box<Self>); }`. `feed` must be non-async and non-blocking (it is called from the audio callback) — an `mpsc::UnboundedSender<Vec<i16>>` into a per-session task is the natural shape.
- Keep `health()` (Swift has no equivalent; the Swift "Test connection" buttons are per-vendor ad-hoc code — see §6.5 — and `health()` is a strictly better factoring, so keep it).
- Keep `capabilities()` — Swift has no such thing, but it is the Rust port's mechanism for the same decisions Swift hardcodes (polish gating, keyterm vs. context blob, local post-processing). Add `streaming: bool` and `accepts_context_at_stop: bool` so Claude Voice's start-time-only keyterms are expressible.
- `prewarm()` maps to `prewarmConnection()`; add `cancel_prewarm()`.
- `TranscribeRequest` becomes the `stop()`-time `DictationContext`: `app`, `ocr_context`, `ax_context`, `dictionary`, `languages`, `transcript_id`. `packets` moves out of it.
- Port `TranscriptionError::shouldFallback` as a distinct predicate on `ProviderError`. Today Rust has only `is_retryable()`. Mapping: `AuthFailed | ConnectionFailed | ServerError | Timeout | NotConfigured | QuotaExceeded | RateLimited` → `should_fallback == true`; `EmptyResult` → `false`.

---

## 2. `DictationVendor`

### 2.1 Verbatim

```swift
enum DictationVendor: String, CaseIterable {
    case wisprFlow = "wispr_flow"
    case openRouter = "openrouter"
    case claudeVoice = "claude_voice"
    case deepgram = "deepgram"

    var displayName: String {
        switch self {
        case .wisprFlow:   return "Wispr Flow"
        case .openRouter:  return "OpenRouter"
        case .claudeVoice: return "Claude Voice"
        case .deepgram:    return "Deepgram"
        }
    }
}
```

| Case | Raw value (persisted) | Display name |
|---|---|---|
| `.wisprFlow` | `"wispr_flow"` | `Wispr Flow` |
| `.openRouter` | `"openrouter"` | `OpenRouter` |
| `.claudeVoice` | `"claude_voice"` | `Claude Voice` |
| `.deepgram` | `"deepgram"` | `Deepgram` |

`allCases` order is declaration order: Flow, OpenRouter, Claude Voice, Deepgram. This order is user-visible — it is the order of the Settings → Provider vendor picker, the status-bar `Provider` submenu, and it determines which vendor `addFallbackStep()` picks by default (§4.4).

Persistence: `AppSettings.activeVendor: String = DictationVendor.wisprFlow.rawValue`, and every `FallbackStep.vendor` is a raw value. Every decode is `DictationVendor(rawValue:) ?? .wisprFlow` — an unknown string silently becomes Wispr Flow, never an error.

Display-name history: commit `72c378e` renamed `"OpenRouter (Gemini)"` → `"OpenRouter"`. Do not reintroduce the parenthetical.

### 2.2 `isReady(session:)` — exact per-vendor logic

```swift
    /// Lightweight, prompt-free check that this vendor has the credentials it
    /// needs to actually run a dictation. Used by the Provider chain UI to
    /// surface "Not signed in" badges before the first failed transcription.
    /// Conservative: returns true unless we can prove the vendor is unauth'd.
    func isReady(session: Session) -> Bool {
        switch self {
        case .wisprFlow:
            return session.isValid
        case .openRouter:
            return SecretsStore.has(.openRouterAPIKey)
                || KeychainStore.hasOpenRouterKeyHint()
                || ProcessInfo.processInfo.environment["WISPR_LIGHTNING_OPENROUTER_KEY"]?.isEmpty == false
        case .claudeVoice:
            let path = NSHomeDirectory() + "/.config/claude/credentials.json"
            return FileManager.default.fileExists(atPath: path)
                || ClaudeCodeCredentialFileLikelyExists()
        case .deepgram:
            return SecretsStore.has(.deepgramAPIKey)
                || ProcessInfo.processInfo.environment["WISPR_LIGHTNING_DEEPGRAM_KEY"]?.isEmpty == false
        }
    }
```

Per vendor:

- **`wisprFlow`** → `session.isValid`, which is:
  ```swift
  guard accessToken != nil else { return false }
  if expiresAt > 0 && Date().timeIntervalSince1970 > expiresAt - 60 { return false }
  return true
  ```
  i.e. a token that expires within the next **60 seconds** already counts as invalid.
- **`openRouter`** → three-way OR, in this order: file-backed `SecretsStore.has(.openRouterAPIKey)`; then the **prompt-free** Keychain catalog probe `KeychainStore.hasOpenRouterKeyHint()` (§6.3); then env var `WISPR_LIGHTNING_OPENROUTER_KEY` non-empty.
- **`claudeVoice`** → probes `$HOME/.config/claude/credentials.json` for existence, then ORs with `ClaudeCodeCredentialFileLikelyExists()`, which is a private free function that **unconditionally returns `true`**:
  ```swift
  /// Some claude CLI versions keep credentials in the Keychain only, not on disk.
  /// We can't probe the Keychain without prompting, so fall back to "unknown ready".
  private func ClaudeCodeCredentialFileLikelyExists() -> Bool {
      // Conservative: we don't have a prompt-free signal, so don't claim "not
      // ready" — show the Claude Voice row without a warning. The user finds
      // out via the Check button if they want to be sure.
      return true
  }
  ```
  **Net effect: `DictationVendor.claudeVoice.isReady(session:)` always returns `true`.** The file probe is dead weight in the boolean; it is kept so the intent survives if a prompt-free Keychain probe ever exists. Claude Voice never shows the "Not signed in" badge.
- **`deepgram`** → `SecretsStore.has(.deepgramAPIKey)` OR env var `WISPR_LIGHTNING_DEEPGRAM_KEY` non-empty. Note the asymmetry with OpenRouter: **no Keychain hint**, because Deepgram's key was never in the Keychain (it shipped after the move to `SecretsStore`).

### 2.3 Environment-variable escape hatches — the complete list

| Variable | Read by | Precedence |
|---|---|---|
| `WISPR_LIGHTNING_OPENROUTER_KEY` | `OpenRouterProvider.apiKey`, `DictationVendor.isReady` | **Highest** — checked before `SecretsStore`, before Keychain migration |
| `WISPR_LIGHTNING_DEEPGRAM_KEY` | `DeepgramProvider.apiKey()`, `DictationVendor.isReady` | **Highest** — checked before `SecretsStore` |
| `VOICE_STREAM_BASE_URL` | `VoiceStreamConfig.defaultBaseURL` | Overrides `wss://api.anthropic.com`. Parsed with `URL(string:)`; an unparseable value falls back to the default |

In all three cases the check is "present and non-empty" (`!env.isEmpty` / `?.isEmpty == false`). There is no env override for the Wispr Flow session or for the Claude Code token.

### 2.4 UI surface

`VendorReadinessBadge` renders **nothing** when `vendor.isReady(session:)` is true; otherwise:

```
Label("Not signed in", systemImage: "exclamationmark.triangle.fill")
  .font(.caption2), horizontal padding 6, vertical padding 2
  .background(Color.orange.opacity(0.18), in: Capsule())
  .foregroundStyle(.orange)
  .help("Set up this vendor in the Accounts tab.")
```

It appears beside the vendor picker in the primary row and in every `FallbackStepRow`.

Status bar (`StatusBarController.buildMenu`): a `Provider` submenu listing all four `DictationVendor.allCases` by `displayName`, `state = .on` for the one matching `settings.activeVendor`, action `selectVendor(_:)` with `representedObject = vendor.rawValue`.

Also in the status bar — this trigger checks the **whole chain**, not just the primary:

```swift
let chainVendors: [String] = [settings.activeVendor] + settings.fallbackChain.map { $0.vendor }
if chainVendors.contains(DictationVendor.wisprFlow.rawValue) && !session.isValid { … }
```
→ pins a `⚠ Wispr Flow sign-in required` item (systemOrange attributed title) at the top of the menu **and** composites an orange badge onto the menu-bar icon. Rationale in the source: *"otherwise a chain like OpenRouter→Flow would silently hit a dead Flow step and the user would never know to re-sign-in."*

### What the Rust port must change
`wl_core::settings::ProviderId` has exactly two variants (`Wispr`, `Deepgram`) with `as_str()` values `"wispr"` / `"deepgram"`. Both the variant set **and the raw strings** are wrong:

- Add `OpenRouter` and `ClaudeVoice`.
- Change the serialized strings to `"wispr_flow"`, `"openrouter"`, `"claude_voice"`, `"deepgram"` so `settings.json` interoperates with the Swift app. `"wispr"` → `"wispr_flow"` needs a migration in the settings loader (map the legacy string, don't error).
- `display_name()` must return exactly `"Wispr Flow"`, `"OpenRouter"`, `"Claude Voice"`, `"Deepgram"`.
- Add `fn is_ready(&self, session: &Session) -> bool` with the four arms above, including the Claude-Voice-always-true behaviour and the env-var checks. Do **not** "fix" the Claude Voice arm into something that returns false — the badge is deliberately suppressed there.
- Enumeration order must be Flow, OpenRouter, Claude Voice, Deepgram wherever the UI iterates.

---

## 3. The four providers

### 3.1 `WisprFlowProvider`

| Property | Value |
|---|---|
| Endpoint | `wss://api.wisprflow.ai/llm/ws` (`Constants.wsURL`) |
| Handshake header | `Encoding: json` |
| Auth | Supabase OAuth bearer, sent **in the first WS message**, not as an HTTP header |
| Transport | WebSocket, text frames, JSON |
| Buffering | Full — packets accumulate in `bufferedPackets` under `bufferLock: NSLock`, uploaded at `stop()` |
| Receive buffer | `task.maximumMessageSize = 10 * 1024 * 1024` (10 MB) |
| Keepalive | `sendPing` every **20.0 s** (`pingInterval`) |
| Chunk size | **500** packets per `append` message (~20 s of audio, ~800 KB encoded) |
| Auth timeout | **10.0 s** |
| Result timeout | `max(15.0, packetCount * 0.04 * 0.5)` seconds |
| Audio encoding | ascii85 (`base64.a85encode`-compatible), `audio_encoding: "wav"`, `byte_encoding: "ascii85"` |

#### Connection

```swift
var request = URLRequest(url: URL(string: Constants.wsURL)!)
request.setValue("json", forHTTPHeaderField: "Encoding")
let task = URLSession.shared.webSocketTask(with: request)
task.maximumMessageSize = 10 * 1024 * 1024
task.resume()
startPinging(task)
```

`prewarmConnection()` creates such a task, stores it in `prewarmedTask` under `prewarmLock`, and additionally, if `!session.isValid`, fires `session.refresh { … }` logging `"Wispr Lightning: Proactive token refresh failed"` on failure.

At `performTranscription` time the prewarmed task is claimed and nil'd under the lock; it is reused **only if `prewarmed.state == .running`**, otherwise it is logged (`"Prewarmed connection stale (state: \(rawValue)), creating fresh one"`), un-pinged, cancelled with `.normalClosure`, and a fresh task is made.

#### Keepalive (B-003, commit `61e9cd6`)

Not a timer — a self-rearming `DispatchWorkItem` on `DispatchQueue.global(qos: .utility)`, tracked in `pingWorkItems: [ObjectIdentifier: DispatchWorkItem]` under `pingLock`. Each firing:
1. bail (and `stopPinging`) if `task.state != .running`;
2. bail if `pingWorkItems[key] !== work` (identity check — the work item was superseded);
3. `task.sendPing { error in if let error { wLogVerbose("WS ping failed: \(error.localizedDescription)") } }`;
4. re-schedule itself at `+20.0 s`.

Ping errors are swallowed by design: *"by then the socket is already closed and the next send/receive will surface the real error to the caller."*

#### Token refresh before upload

```swift
guard session.isValid else {
    NSLog("Wispr Lightning: Token expired, refreshing...")
    session.refresh { success in
        guard success else {
            DispatchQueue.main.async {
                NotificationCenter.default.post(name: .sessionChanged, object: nil)
            }
            completion(.failure(.authFailed(
              "Wispr Flow sign-in expired and refresh failed. Open Settings → Accounts → Wispr Flow and sign in again.")))
            return
        }
        performTranscription(…)
    }
    return
}
```

The `.sessionChanged` post is on main specifically so the status-bar observer can rebuild its menu without a cross-queue UI update.

#### Message 1 — `auth` (verbatim key structure)

```json
{
  "type": "auth",
  "access_token": "<session.accessToken ?? \"\">",
  "app": "<appInfo[\"type\"] ?? \"other\", lowercased>",
  "context": {
    "app": {
      "name":      "<appInfo[\"name\"] ?? \"\">",
      "bundle_id": "<appInfo[\"bundle_id\"] ?? \"\">",
      "type":      "<same lowercased type>",
      "url":       "<appInfo[\"url\"] ?? \"\">"
    },
    "ax_context":  [ …context.axContext… ],
    "ocr_context": [ …context.ocrContext… ],
    "dictionary_context":      [ …dictionaryStore.getVocabularyPhrases() ?? []… ],
    "dictionary_replacements": { …getReplacements() ?? [:]… },
    "dictionary_snippets":     { "<trigger>": ["<expansion>"] },
    "user_first_name": "<session.userFirstName ?? \"\">",
    "user_last_name":  "<session.userLastName ?? \"\">",
    "textbox_contents": {},
    "content_text": "",
    "variable_names": [],
    "file_names": []
  },
  "personalization_style_settings": { … } ,
  "language": [ …settings.languages… ],
  "metadata": {
    "session_id":             "<session.sessionId>",
    "environment":            "PRODUCTION",
    "client_platform":        "darwin",
    "client_version":         "1.4.549",
    "transcript_entity_uuid": "<UUID().uuidString>"
  },
  "pipeline": ["transcribe", "format"],
  "job_selectors": [],
  "cleanup_level": "<settings.autoCleanupLevel>",
  "command_mode": <settings.commandModeEnabled>,
  "debug_mode": false,
  "use_staging_baseten": false,
  "prefix_is_written": <!context.axContext.isEmpty>,
  "hyperlink_on": <settings.hyperlinkOn>
}
```

Conditional fields, exactly:
- `pipeline` = `settings.aiFormatting ? ["transcribe", "format"] : ["transcribe"]`
- `personalization_style_settings` = `settings.styleDetectionEnabled ? settings.personalizationStyles : [:]` (the default `personalizationStyles` is `["work": "default", "email": "default", "personal": "default", "other": "default"]`)
- `job_selectors` = `settings.creatorMode ? ["creator"] : []`
- `dictionary_snippets` wraps each snippet value in a **single-element array**: `.mapValues { [$0] }`
- `prefix_is_written` is `true` iff `axContext` is non-empty

Log lines: `"WS sending auth — token: \(first 8 chars)..., app: \(appType), pipeline: \(joined by ,)"` (verbose).

#### Auth response

A **10.0 s** `DispatchWorkItem` watchdog is armed immediately after the auth send:

```swift
let authTimeout = DispatchWorkItem { … }
DispatchQueue.global().asyncAfter(deadline: .now() + 10.0, execute: authTimeout)
```

Rationale, verbatim: *"without this, a hung server (upgrade succeeded but no auth response) parks the recording in Processing until URLSession's ~30s default resource timeout … 10s is well past normal handshake (~700ms) but short enough that the chain advances quickly when the backend is broken."* On fire: log `"Wispr Flow: auth response timed out — falling back"`, stop pinging, `cancel(with: .goingAway)`, `.failure(.timeout)`.

The single `receive` cancels the watchdog first thing, then:
- parses the frame as JSON, reads `json["status"] as? String ?? "unknown"`, logs `"WS auth response: status=\(statusWord)"`;
- `status == "auth"` → log `"WebSocket authenticated"`, proceed to send audio;
- anything else, or a non-string frame → log `"WebSocket auth failed — unexpected response"` / `"… non-string message received"`, `cancel(with: .internalServerError)`, `.failure(.authFailed("Wispr Flow rejected the WebSocket auth. Open Settings → Accounts → Wispr Flow and sign in again."))`;
- receive `.failure` → `.failure(.connectionFailed)`.

#### Audio preparation (parallel with auth)

Encoding runs on `encodingQueue` (`DispatchQueue(label: "com.wisprlightning.encode", qos: .userInitiated)`) **concurrently with the auth round-trip**, joined by a `DispatchGroup` whose `notify` fires `sendAudio` on `encodingQueue`. If `cachedEncoding?.packetCount == packets.count`, the cached `PreparedAudio` is reused and no group is created.

`prepareAudio` per packet:
```
encodedPackets.append(ascii85Encode(packet))
sampleCount = packet.count / 2
sumSquares  = Σ (Int16 sample as Double)²
rms         = sqrt(sumSquares / sampleCount)
volumes.append( (rms / 32768.0 * 10000).rounded() / 10000 )
```
i.e. RMS normalized to 0…1 and rounded to **4 decimal places**.

`ascii85Encode` is a hand-rolled `base64.a85encode` clone: 4 bytes → 32-bit big-endian value → 5 digits base-85 offset by `33` (`'!'`); an all-zero full group emits the single byte `0x7A` (`'z'`); a partial trailing group of `n` bytes is zero-padded to 4 and emits `n + 1` digits. No `<~ ~>` delimiters, no line wrapping.

#### Message 2..N — `append`

```json
{
  "type": "append",
  "audio_packets": {
    "packets":        ["<ascii85>", …],
    "volumes":        [0.0123, …],
    "packet_duration": 0.04,
    "audio_encoding": "wav",
    "byte_encoding":  "ascii85"
  },
  "position": <offset, in packets>,
  "final": <bool>
}
```

`chunkSize = 500`. Chunks are sent **strictly sequentially** — `sendNextChunk` recurses from inside the previous send's completion handler, never fires them concurrently. `position` is the packet offset of the chunk start (0, 500, 1000, …). `final = (offset + 500 >= totalPackets)`. `packet_duration` is `Double(Constants.chunkDurationMs) / 1000.0` = `0.04`. Verbose log per chunk: `"WS sending chunk \(offset)..<\(end) of \(totalPackets) (\(appendString.count) bytes, final=\(isFinal))"`.

Any send error → log `"Wispr Lightning: WS chunk send failed: %@"` → `.failure(.connectionFailed)`.

#### Message N+1 — `commit`

```json
{ "type": "commit", "total_packets": <int> }
```

Then `NSLog("Wispr Lightning: Audio sent — %d packets in %d chunks, waiting for transcription...", totalPackets, chunkCount)` where `chunkCount = (totalPackets + 499) / 500`.

#### Result receive loop

Timeout: `responseTimeout(for:) = max(15.0, Double(packetCount) * 40 / 1000 * 0.5)` — i.e. **15 s floor, otherwise half the recording's wall duration**. Armed on `DispatchQueue.global(qos: .userInitiated)`. On fire: `NSLog("Wispr Lightning: WebSocket response timed out after %.0fs", timeout)`, stop pinging, `cancel(with: .abnormalClosure)`, `.failure(.timeout)`.

Frames are dispatched on `json["status"]`:

| `status` | Handling |
|---|---|
| `"text"` | `body = json["body"] as? [String: Any] ?? [:]`; `llmText = body["llm_text"] as? String`; `asrText = body["asr_text"] as? String`; `isFinal = json["final"] as? Bool ?? false`; `resultText = llmText ?? asrText ?? ""`. Logs `"Got %@ transcript: %d chars"` with `final`/`partial`. If `isFinal`: build `TranscriptResult(id: transcriptUUID, asrText: asrText, formattedText: llmText, duration: packetCount*0.04, numWords: resultText.split(" ").count)`, stop pinging, `cancel(with: .normalClosure)`, then **`.failure(.emptyResult)` if `resultText.isEmpty`** else `.success`. If not final: keep receiving. |
| `"error"` | `errorDetail = json["error"] as? String ?? "unknown"`, log `"Server error: %@"`, `cancel(with: .internalServerError)`, `.failure(.serverError(errorDetail))` |
| `"info"` | log `"Server info: %@"` with `json["message"]`, keep receiving |
| anything else | keep receiving |

Receive `.failure` → log `"WS receive failed: %@"` → `.failure(.connectionFailed)`. Note: a `.success` frame that is **not** a parseable string-JSON object silently ends the loop without completing — the outer timeout is the only backstop.

Both completion paths go through `SafeCompletion` (`performTranscription`) plus a second inner lock-and-bool in `receiveResultWithTimeout`.

#### What the Rust port must change
`crates/wl-providers/src/wispr.rs` already implements this vendor over WSS. Deltas to verify against the above:
- The **10 s auth watchdog** is a `8a81d74`-era addition; confirm it exists.
- Keepalive must be **20.0 s** `sendPing` (WS ping frame, not a JSON keepalive message).
- `maximumMessageSize` → the equivalent read-frame cap must be at least 10 MiB.
- The chunker must be **500 packets**, sequential, with `position` in packets.
- `prefix_is_written` must be `!ax_context.is_empty()`.
- `.emptyResult` when the final text is empty, even though the frame said `status == "text"`, `final == true`.

### 3.2 `OpenRouterProvider`

| Property | Value |
|---|---|
| Endpoint | `https://openrouter.ai/api/v1/chat/completions` |
| Method | `POST` |
| Auth | `Authorization: Bearer <key>` |
| Extra headers | `Content-Type: application/json`, `X-Title: Wispr Lightning`, `HTTP-Referer: https://github.com/cefege/wispr` |
| Timeout | `request.timeoutInterval = 90` |
| Transport | Plain HTTPS, `URLSession.shared.dataTask`, **no streaming** (`"stream": false`) |
| Buffering | Full; WAV built at `stop()` |
| Default model | `"google/gemini-2.5-flash-lite"` |
| Model list endpoint | `https://openrouter.ai/api/v1/models`, timeout `20`, no auth |
| Key test endpoint | `https://openrouter.ai/api/v1/auth/key`, timeout `15` |

#### Key resolution (`private var apiKey: String?`)

```
1. ProcessInfo.environment["WISPR_LIGHTNING_OPENROUTER_KEY"]  (if non-empty)
2. SecretsStore.read(.openRouterAPIKey)
3. KeychainStore.read(.openRouterAPIKey)  ← migration path, MAY prompt
     if SecretsStore.write(.openRouterAPIKey, migrated) { KeychainStore.delete(.openRouterAPIKey) }
     else { wLog("OpenRouter: failed to migrate key to SecretsStore; leaving Keychain copy intact") }
     return migrated
4. nil
```

Step 3's conditional delete is the `87b24c1` fix: an unconditional delete after a failed file write wiped the key from **both** stores.

Missing key → log `"OpenRouter: no API key — open Settings → Accounts → OpenRouter and paste your key"` and `.failure(.authFailed("OpenRouter has no saved API key. Open Settings → Accounts → OpenRouter and paste one from openrouter.ai/keys."))`.

#### Model resolution (`private var model: String`)

```
1. modelOverride?.trimmingCharacters(in: .whitespaces), if non-empty   ← per-chain-step override
2. settings.openRouterModel.trimmingCharacters(in: .whitespaces), if non-empty
3. "google/gemini-2.5-flash-lite"
```

`modelOverride` is an `init(settings:modelOverride:)` parameter (default `nil`), set only by the fallback chain (§4). Its whole purpose, from B-012: *"so the same vendor can appear multiple times in the chain with different models (e.g. Flow → Claude Voice → OpenRouter:flash-lite → OpenRouter:pro)."*

#### System prompt (verbatim, including the line continuations)

```
You are a dictation transcriber. Transcribe the audio with light cleanup: fix punctuation, capitalization, and remove filler words (um, uh, like, you know). Preserve the speaker's word choice and tone.

You are NOT a chatbot. If the audio contains a question or request, TRANSCRIBE it — do not answer it. Output ONLY the cleaned transcript, nothing else.
```

(The Swift literal is a `"""` block with trailing `\` continuations; the rendered string is the two paragraphs above separated by a blank line.)

When the dictionary has vocabulary phrases, up to **40** of them (`words.prefix(40)`) are appended to the system content as:

```
\n\nThe speaker frequently uses these proper nouns or jargon — spell them as written: <a, b, c>.
```

(joined with `", "`, terminated by a period).

#### Request body (verbatim keys)

```json
{
  "model": "<resolved model>",
  "stream": false,
  "messages": [
    { "role": "system", "content": "<systemPrompt + customWordsLine>" },
    { "role": "user", "content": [
        { "type": "input_audio", "input_audio": { "data": "<base64 WAV>", "format": "wav" } }
    ]}
  ]
}
```

`data` is `AudioEncoding.base64WavString(from: packets)` — a full RIFF/WAVE file, base64'd, **not** a data URI, no `data:audio/wav;base64,` prefix.

WAV header (`AudioEncoding.wavData`, 44 bytes, all multi-byte fields little-endian):

| Offset | Bytes | Value |
|---|---|---|
| 0 | `52 49 46 46` | `"RIFF"` |
| 4 | u32 | `36 + dataSize` |
| 8 | `57 41 56 45` | `"WAVE"` |
| 12 | `66 6D 74 20` | `"fmt "` |
| 16 | u32 | `16` |
| 20 | u16 | `1` (PCM) |
| 22 | u16 | `1` (mono) |
| 24 | u32 | `16000` |
| 28 | u32 | `32000` (`sampleRate * 2`, byte rate) |
| 32 | u16 | `2` (block align) |
| 34 | u16 | `16` (bits per sample) |
| 36 | `64 61 74 61` | `"data"` |
| 40 | u32 | `dataSize = packets.count * 1280` |

Note `dataSize` is computed as `packets.count * Constants.chunkSamples * 2` — it assumes every packet is exactly 1280 bytes, which `AudioRecorder` guarantees.

Pre-flight log: `"OpenRouter: sending ~\(approxWavKB)KB WAV, \(%.1f duration)s, model=\(model)"` where `approxWavKB = (44 + packets.count * 640 * 2) / 1024`.

#### Response parsing

Success path requires, in order: `data` non-nil; HTTP status in `200..<300`; body parses as `[String: Any]`; `json["choices"] as? [[String: Any]]`; `.first`; `["message"] as? [String: Any]`; `["content"] as? String`. Then `content.trimmingCharacters(in: .whitespacesAndNewlines)`.

| Failure | Error |
|---|---|
| transport error | log `"OpenRouter: network error — \(desc)"` → `.connectionFailed` |
| `data == nil` | `.connectionFailed` |
| non-2xx | log `"OpenRouter: HTTP \(code) — \(first 400 chars)"` → `classifyError(statusCode:body:)` |
| body not JSON | log `"OpenRouter: response was not JSON"` → `.serverError("malformed JSON response")` |
| missing `choices[0].message.content` | log `"OpenRouter: no content in response — \(first 400 chars)"` → `.serverError("no content")` |
| trimmed content empty | `.emptyResult` |

Success log: `"OpenRouter: got \(text.count) chars in \(%.1f elapsed)s"`. Result: `TranscriptResult(id: UUID().uuidString, asrText: text, formattedText: text, duration: packets.count*0.04, numWords: text.split(" ").count)` — **`asrText` and `formattedText` are the same string**.

#### `classifyError(statusCode:body:)` — verbatim mapping

Body parse is capped: `let parseable = body.count <= 64 * 1024 ? body : body.prefix(64 * 1024)` (*"so a 1MB CDN error page doesn't burn time"*). `errorBlob = json?["error"] as? [String: Any]`, `serverMessage = errorBlob?["message"] as? String ?? fallbackText ?? ""`.

`fallbackText` is only used when the body looks like text: read the first 50 bytes as UTF-8, trim whitespace/newlines, and **return `nil` if it starts with `<`** (HTML page); otherwise take the first 200 bytes, replace `"\n"` with `" "`, trim spaces.

| Status | Returned error | Message (verbatim) |
|---|---|---|
| 401, 403 | `.authFailed` | `OpenRouter: your API key was rejected (HTTP <code>). Open Settings → Accounts → OpenRouter and paste a fresh key from openrouter.ai/keys.` |
| 402 | `.authFailed` | `OpenRouter: out of credits (HTTP 402). Add funds at openrouter.ai/credits, then try again. <serverMessage>` |
| 429 | `.serverError` | `OpenRouter: rate limited (HTTP 429). <serverMessage>` |
| 400, 404 | `.authFailed` | `OpenRouter: HTTP <code> — <serverMessage>. The model id may no longer exist; pick a different one in Settings → Provider.` |
| 500…599 | `.serverError` | `OpenRouter: server error HTTP <code>. <serverMessage>` |
| default | `.serverError` | `OpenRouter: HTTP <code>: <serverMessage>` |

The 400/404 → `.authFailed` mapping is deliberate and **not** a bug: `.authFailed` is `isRetryable == false` but `shouldFallback == true`, so a stale model id advances the chain immediately instead of burning two pointless in-place retries. The source says so: *"Non-retryable since the chain would hit the same error; route as authFailed-style guidance so the fallback chain advances rather than auto-retrying twice first."* Same reasoning for 402 (out of credits — retrying costs money and cannot succeed).

#### Model picker (`OpenRouterModels`)

`GET https://openrouter.ai/api/v1/models`, `timeoutInterval = 20`, no auth header. Parse `json["data"] as? [[String: Any]]`. For each entry, keep only those where `m["architecture"]["input_modalities"] as? [String]` **contains `"audio"`**. Then:

- `id = m["id"] as? String` (required, else skip)
- `name = m["name"] as? String ?? id`
- `pricing = m["pricing"] as? [String: Any] ?? [:]`
- `promptPerMTokens = parsePrice(pricing["prompt"]) ?? 0`, `× 1_000_000`
- `completionPerMTokens = parsePrice(pricing["completion"]) ?? 0`, `× 1_000_000`
- `audioPerMTokens = parsePrice(pricing["audio"]).map { $0 * 1_000_000 }` (Optional)

`parsePrice` accepts a `Double` directly, or a `String` (OpenRouter sends e.g. `"0.00000025"`); `""` and `"-"` → `nil`.

Sort: ascending `promptPerMTokens`, tie-broken by ascending `id`. Completion is delivered on `DispatchQueue.main`. Malformed list → `NSError(domain: "OpenRouterModels", code: 1, "Malformed model list")`.

Picker label: `"\(name) — \(inStr) / \(outStr)"` where each price is `"free"` when `<= 0`, else `String(format: "$%.2f", v)`.

`loadOpenRouterModels(force:)` is idempotent per session: it returns early if the state is already `.loading` or `.loaded`. When the list hasn't loaded, the picker shows a disabled `Text("Loading models…").tag("loading-placeholder")`. When loaded but the configured model isn't in the list, an extra row `Text("Custom: \(vm.openRouterModel)").tag(vm.openRouterModel)` is appended so the selection isn't silently lost.

### 3.3 `ClaudeVoiceProvider` + `VoiceStream`

| Property | Value |
|---|---|
| Base URL | `wss://api.anthropic.com` (overridable via `VOICE_STREAM_BASE_URL`) |
| Path | `/api/ws/speech_to_text/voice_stream` |
| Auth | `Authorization: Bearer <accessToken>` from the `Claude Code-credentials` Keychain item |
| Transport | WebSocket. **Binary frames for PCM**, text frames for control JSON |
| Buffering | Pre-open only (handshake window) |
| Keepalive | `{"type":"KeepAlive"}` every **8 s** |
| Finalize wait | **2.0 s** cap |
| Scheme rule | `components.scheme = baseURL.scheme == "wss" ? "wss" : "ws"` |

#### URL construction (verbatim query items, in order)

```swift
var items: [URLQueryItem] = [
    .init(name: "encoding",              value: "linear16"),
    .init(name: "sample_rate",           value: String(Constants.sampleRate)),   // "16000"
    .init(name: "channels",              value: "1"),
    .init(name: "endpointing_ms",        value: "300"),
    .init(name: "utterance_end_ms",      value: "1000"),
    .init(name: "language",              value: language),
    .init(name: "use_conversation_engine", value: "true"),
    .init(name: "stt_provider",          value: "deepgram-nova3"),
]
for term in keyterms { items.append(.init(name: "keyterms", value: term)) }
```

Note the parameter name is **`keyterms`** (plural), repeated once per term — different from Deepgram's own `keyterm` (singular). `language` is `settings.languages.first ?? "en"`.

#### Request headers (all four required)

```
Authorization:              Bearer <accessToken>
User-Agent:                 wispr-lightning/0.1 (macOS)
x-app:                      cli
anthropic-client-platform:  claude_code_cli
```

Source comment on the last one: *"Required by the Claude Code CLI variant of the endpoint switch per verified binary 2.1.119. Missing this header returns a 4xx."* `defaultBaseURL`'s comment: *"api.anthropic.com is the registered API surface and matches the verified Claude Code 2.1.119 binary behavior."*

Connect log: `"Claude Voice: connecting to \(url.absoluteString)"`.

#### 8 s keepalive

```swift
/// 8s keepalive — verified from Claude Code binary 2.1.119. Wispr Flow uses 20s
/// for its load balancer; this is a different endpoint. Don't bump.
private let claudeVoiceKeepAliveInterval: TimeInterval = 8
```

A `DispatchSource.makeTimerSource(queue: queue)` scheduled `deadline: .now() + 8, repeating: 8`, started in `didOpenWithProtocol`. Each tick sends the **text** frame `{"type":"KeepAlive"}` (only if `isOpen` and the task exists). One `KeepAlive` is also sent immediately on open, before flushing the pre-open buffer. Cancelled in `close()` and in `didCloseWith`.

#### PCM framing

`send(pcm: Data)` hops onto the private serial queue `WisprLightning.VoiceStream.queue`:

```swift
guard let self, let task = self.task, !self.didCloseStream else { return }
if self.isOpen {
    task.send(.data(pcm)) { error in if let error { wLogVerbose("Claude Voice: send error — \(error.localizedDescription)") } }
} else {
    self.preOpenBuffer.append(pcm)
}
```

Each packet is one **binary** WS frame of exactly 1280 bytes of raw s16le PCM. No header, no length prefix, no base64, no JSON wrapper.

**Pre-open buffering (commit `80ee5ec`)** — load-bearing, verbatim rationale: *"AVAudioEngine starts producing packets ~150ms after `start()`, but the WS open (TCP + TLS + Upgrade) takes 700-1500ms. Without buffering here, the first ~1s of speech is dropped on the floor before isOpen flips true, and the server never sees enough audio to emit a final."* On open, the buffer is drained in order (append and send happen on the same serial queue, so order is preserved) and logged: `"Claude Voice: flushing \(buffered.count) pre-open packets"`.

#### Server message handling

A frame is decoded as UTF-8 (from `.data` or `.string`), parsed as a JSON object, and switched on `obj["type"] as? String ?? ""`:

| `type` | Handling |
|---|---|
| `"TranscriptText"` | `t = obj["data"] as? String`; if non-empty, set `lastInterim = t` (bounced onto `queue` to avoid racing `resolveFinalization`) and call `delegate.voiceStream(_:didReceiveInterim:)` |
| `"TranscriptEndpoint"` | on `queue`: `flushLastInterimAsFinal()` then `resolveFinalization()` |
| `"TranscriptError"` | `msg = obj["description"] as? String ?? obj["error_code"] as? String ?? "unknown transcription error"` → `didFailWith(msg, fatal: false)` |
| `"error"` | `msg = obj["message"] as? String ?? "server error"` → `didFailWith(msg, fatal: false)` |
| default | ignored |

There is **no separate "final transcript" frame**. `flushLastInterimAsFinal()` promotes the last interim string to a final and clears it; that is the only way a final is ever produced. `TranscriptEndpoint` marks the end of an utterance, and the provider concatenates the finals from all utterances in the session.

#### `finalize()` — `stop()` path

`async`, wrapping a `CheckedContinuation`. On `queue`:
1. no task → resume immediately;
2. `didCloseStream = true`;
3. `task.state != .running` → set `pendingFinalization`, `resolveFinalization()`, return (*"If the socket isn't running we're not going to get a TranscriptEndpoint — resolve now instead of waiting 2s"*);
4. `isOpen` → send text frame `{"type":"CloseStream"}`;
5. `!isOpen` → set `pendingFinalization`, `resolveFinalization()`, return (pre-open finalize; nothing to flush);
6. otherwise set `pendingFinalization` and arm `DispatchQueue.global().asyncAfter(deadline: .now() + 2.0)` → `resolveFinalization()`, captured **weakly** (*"without it the 2s closure pins the VoiceStream alive across cancellation … keeps the delegate's ClaudeVoiceProvider alive for an extra 2s after the user has moved on"*).

`resolveFinalization()` is idempotent (guards on `pendingFinalization != nil`), calls `flushLastInterimAsFinal()`, `close()`, then `cont.resume()`.

`close()`: cancel keepalive timer, `task?.cancel(with: .normalClosure, reason: nil)`, `task = nil`, `isOpen = false`, drop `preOpenBuffer`.

#### `didCloseWith` handling

On `queue`: `flushLastInterimAsFinal()`, `isOpen = false`, cancel keepalive, `delegate.voiceStreamDidClose`, log `"Claude Voice: didCloseWith code=\(closeCode.rawValue) reason=\(describeCloseReason(reason))"`. Then, if `closeFailureMessage(code:reason:)` returns non-nil, `didFailWith(message, fatal: false)`:

```swift
if code == .normalClosure || code == .noStatusReceived { return nil }
return "Closed with code \(code.rawValue) \(reasonStr)"
```

Finally `resolveFinalization()`.

#### Receive-failure classification

```swift
enum ReceiveFailureAction: Equatable { case suppress, report(message: String) }

if didCloseStream { return .suppress }
if let urlError = error as? URLError, urlError.code == .cancelled { return .suppress }
return .report(message: "Receive error: \(error.localizedDescription)")
```

`.suppress` → `wLogVerbose("Claude Voice: receive ended after clean close — \(desc)")`. `.report` → `didFailWith(message, fatal: true)`. Either way, `close()`.

#### Provider-level session management

`beginSession()` (on the provider's serial queue `WisprLightning.ClaudeVoiceProvider`):

1. clear `finals`, `packetCount = 0`, `failureMessage = nil`, `failureIsAuth = false`, close any prior stream;
2. `token = try ClaudeCodeKeychain.read()`. On throw: log `"Claude Voice: \(error)"`, set `failureMessage = "Run \`claude /login\` in a terminal, then click Re-check in Settings → Accounts → Claude Voice."`, `failureIsAuth = true`, `stream = nil`, `inSession = false`, return;
3. `token.isExpired` → `ClaudeCodeKeychain.clearAllCaches()`, log `"Claude Voice: token expired — run \`claude /login\`"`, `failureMessage = "Claude Code session expired. Run \`claude /login\` in a terminal, then click Re-check in Settings → Accounts → Claude Voice."`, `failureIsAuth = true`, `inSession = false`, return;
4. build keyterms (§7.2) — `ClaudeVoiceKeyTerms.extract(from: pendingOcrLines, limit: 20)`, then append dictionary phrases not already present, hard-stopping at **20** total;
5. `language = settings.languages.first ?? "en"`;
6. construct `VoiceStreamConfig`, `VoiceStream`, set `delegate = self`, `inSession = true`, `try voice.connect()`. On throw: log `"Claude Voice: failed to connect — \(desc)"`, **`voice.close()`** (*"Tear the stream down so its URLSession (which retains the VoiceStream as its delegate) doesn't leak"*), `stream = nil`, `inSession = false`, `failureMessage = "Failed to open Claude Voice stream: \(desc)"` (note: `failureIsAuth` stays false here).

`stop(context:completion:)`: if `!inSession || stream == nil`, complete immediately with the recorded reason — `.authFailed(msg)` when `failureIsAuth`, else `.serverError(msg)`; the default msg is `"Claude Voice is not signed in. Run \`claude /login\` in a terminal."`. Otherwise `await stream.finalize()` then `deliverResult`.

`deliverResult`: `collected = finals.joined(separator: " ")`, trimmed. Then:
- `failure != nil && cleaned.isEmpty` → if `failureIsAuth`: `ClaudeCodeKeychain.clearAllCaches()` and `.failure(.authFailed(nil))` — **nil**, deliberately, so the generic message shows and the next attempt re-reads the upstream item; else `.failure(.serverError(failure))`;
- `cleaned.isEmpty` → `.failure(.emptyResult)`;
- else `.success(TranscriptResult(id: UUID().uuidString, asrText: cleaned, formattedText: cleaned, duration: packetCount*0.04, numWords: …))`.

Note that a **non-empty transcript wins over a recorded failure** — a mid-stream `TranscriptError` that still produced text is delivered as a success.

Auth-error heuristic on delegate failures:

```swift
private static func looksLikeAuthError(_ message: String) -> Bool {
    let lower = message.lowercased()
    return lower.contains("unauthorized")
        || lower.contains("401")
        || lower.contains("403")
        || lower.contains("invalid token")
        || lower.contains("invalid_token")
        || lower.contains("auth")
}
```

(The bare `"auth"` substring is intentionally broad.)

`didReceiveInterim` is a **deliberate no-op** in the provider: *"Lightning's pill doesn't render interim transcripts today; we only collect finals. Keeping this hook so future UI can show partials."*

`cancel()` closes the stream, nils it, clears finals / packetCount / failure state / `inSession`.

Other logs: `"Claude Voice: stream opened"` (info), `"Claude Voice: stream closed"` (verbose), `"Claude Voice: \(message) (fatal=\(fatal))"`.

#### `Claude Code-credentials` Keychain ownership

```swift
/// Item written by the `claude` CLI. Read once, then mirrored into
/// SecretsStore (file) so future reads don't trigger the cross-app
/// password prompt.
static let upstreamService = "Claude Code-credentials"
```

Query — note there is **no `kSecAttrAccount`**:

```swift
[kSecClass: kSecClassGenericPassword,
 kSecAttrService: "Claude Code-credentials",
 kSecReturnData: true,
 kSecMatchLimit: kSecMatchLimitOne]
```

Lightning **never writes to this item.** Ownership belongs to the `claude` CLI; Lightning is a read-only consumer.

Payload shape:

```json
{ "claudeAiOauth": {
    "accessToken": "...",
    "refreshToken": "...",
    "expiresAt": 1234567890123,
    "scopes": ["..."],
    "subscriptionType": "..." } }
```

Decoded via `ClaudeCodeCredentialsEnvelope { let claudeAiOauth: ClaudeCodeOAuthToken }`. `expiresAt` is **milliseconds** since epoch:

```swift
var isExpired: Bool {
    guard let expiresAt else { return false }        // absent → never expires
    let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
    return nowMs >= expiresAt
}
```

Note: **no clock-skew margin** — unlike `Session.isValid`'s 60 s buffer.

Read cascade (`read(forceRefresh: Bool = false)`):

1. `!forceRefresh` → in-process `cachedToken` if non-nil and not expired;
2. `!forceRefresh` → `readMirror()` (`SecretsStore.read(.claudeCodeTokenMirror)`, JSON-decoded) if not expired;
3. `readUpstream()` — the cross-app Keychain read, **the only prompting call**; then `writeMirror(token)`, `deleteLegacyMirror()`, populate `cachedToken`.

Errors:

```swift
enum ClaudeCodeKeychainError: Error, CustomStringConvertible {
    case itemNotFound        // "No 'Claude Code-credentials' item in Keychain. Run `claude /login` first."
    case readFailed(OSStatus) // "Keychain read failed (OSStatus \(status))."
    case decodeFailed(String) // "Could not decode credentials JSON: \(msg)"
}
```

`errSecItemNotFound` → `.itemNotFound`; any other non-`errSecSuccess`, or a non-`Data` result → `.readFailed(status)`.

Legacy mirror cleanup: service `"com.wisprlightning"`, account `"claude_code.cached_token"`, deleted with `SecItemDelete`. Rationale: *"Each signed rebuild had a different cdhash and the Keychain ACL re-prompt followed every install. The mirror now lives in SecretsStore which has none of that fragility."*

`clearAllCaches()` = drop in-process cache + `SecretsStore.delete(.claudeCodeTokenMirror)` + delete the legacy Keychain mirror. Called by Settings "Re-check" and by the provider on any auth-class failure.

`isCLIInstalled` probes, in order, `isExecutableFile(atPath:)` on:
```
/usr/local/bin/claude
/opt/homebrew/bin/claude
$HOME/.local/bin/claude
$HOME/.npm/bin/claude
$HOME/.bun/bin/claude
```
then every `dir` in `$PATH` split on `":"` for `"\(dir)/claude"`. It deliberately never executes the binary.

### 3.4 `DeepgramProvider` (head commit `8a81d74`)

| Property | Value |
|---|---|
| Endpoint | `wss://api.deepgram.com/v1/listen` |
| Auth | `Authorization: Token <apiKey>` (HTTP header on the WS upgrade) |
| Request timeout | `request.timeoutInterval = 30` (upgrade) |
| Transport | **WebSocket streaming**, binary PCM frames + JSON control frames |
| Model | hard-coded `nova-3` |
| Keepalive | `{"type":"KeepAlive"}` every **5 s**, skipped if a packet was sent within **4.5 s** |
| Finalize | `{"type":"Finalize"}` on `stop()`, **3.0 s** timeout |
| Close | `{"type":"CloseStream"}`, then teardown after **0.2 s** |
| Keyterm cap | **50** phrases |

#### Key resolution

```swift
private static func apiKey() -> String? {
    if let env = ProcessInfo.processInfo.environment["WISPR_LIGHTNING_DEEPGRAM_KEY"], !env.isEmpty { return env }
    return SecretsStore.read(.deepgramAPIKey)
}
```

No Keychain path at all. Missing key → `failureMessage = "Deepgram has no saved API key. Open Settings → Accounts → Deepgram and paste one from console.deepgram.com."`, `failureIsAuth = true`, log `"Deepgram: no API key — open Settings → Accounts → Deepgram"`.

#### URL construction (verbatim, in order)

```swift
URLComponents(string: "wss://api.deepgram.com/v1/listen")
items = [
    ("model",           "nova-3"),
    ("encoding",        "linear16"),
    ("sample_rate",     "16000"),
    ("channels",        "1"),
    ("smart_format",    "true"),
    ("punctuate",       "true"),
    ("interim_results", "false"),
    ("mip_opt_out",     "true"),
]
```

`mip_opt_out=true` rationale, verbatim: *"Privacy: opt out of Deepgram's Model Improvement Program so dictated audio isn't retained for training. No pricing impact on the Nova-3 streaming tier."*

Then language, switching on `settings.deepgramLanguage`:

| Value | Appended |
|---|---|
| `"__auto__"` (`DeepgramLanguage.autoDetectCode`) | `detect_language=true` |
| `"__multi__"` (`DeepgramLanguage.multiCode`) | `language=multi` |
| anything else | `language=<code>`, with empty → `"en"` |

Then keyterms:

```swift
if let phrases = dictionaryStore?.getVocabularyPhrases() {
    for phrase in phrases.prefix(50) {
        items.append(URLQueryItem(name: "keyterm", value: phrase))
    }
}
```

Singular **`keyterm`**, repeated. Source comment: *"Nova-3 supports up to 500 tokens. Capped at 50 phrases … Repeated `keyterm=…` so each phrase is boosted independently rather than fused into one space-delimited cohesive unit."*

Connect log: `"Deepgram: connecting to \(url.host ?? "?") model=nova-3 language=\(settings.deepgramLanguage)"`.

#### Language enum

```swift
enum DeepgramLanguage {
    static let autoDetectCode = "__auto__"
    static let multiCode      = "__multi__"
    static let defaultCode    = "en"
    static let entries: [Entry]   // 35 BCP-47 entries, sorted by English name
}
```

The 35 entries, verbatim `(code, name)`:

```
bg Bulgarian · ca Catalan · zh Chinese · cs Czech · da Danish · nl Dutch ·
en English · et Estonian · fi Finnish · nl-BE Flemish · fr French · de German ·
de-CH German (Switzerland) · el Greek · hi Hindi · hu Hungarian · id Indonesian ·
it Italian · ja Japanese · ko Korean · lv Latvian · lt Lithuanian · ms Malay ·
no Norwegian · pl Polish · pt Portuguese · ro Romanian · ru Russian · sk Slovak ·
es Spanish · sv Swedish · th Thai · tr Turkish · uk Ukrainian · vi Vietnamese
```

`displayName(for:)`: `"__auto__"` → `"Auto-detect"`; `"__multi__"` → `"Multilingual (code-switching)"`; a known code → `"\(name) (\(code))"`; unknown → the code itself.

#### Streaming lifecycle

`feed(packet:)` on the provider's serial queue `WisprLightning.DeepgramProvider`:
```swift
self.packetCount += 1
if self.isOpen, let task = self.task {
    task.send(.data(packet)) { _ in }
    self.lastSendAt = Date()
} else {
    bufferLock.lock(); bufferedPackets.append(packet); bufferLock.unlock()
}
```
Same pre-open buffering as Claude Voice, same rationale in the comment: *"Same gotcha that bit ClaudeVoiceProvider."* Flushed in `didOpenWithProtocol` (log `"Deepgram: stream opened"`), then `lastSendAt = Date()` if anything was flushed.

Keepalive:
```swift
timer.schedule(deadline: .now() + 5, repeating: 5)
timer.setEventHandler {
    if Date().timeIntervalSince(self.lastSendAt) >= 4.5 {
        self.task?.send(.string(#"{"type":"KeepAlive"}"#)) { _ in }
    }
}
```
Comment: *"5s cadence is well inside Deepgram's 10s idle window."* Started in `beginSession()` **immediately after `resume()`** — i.e. before the socket opens.

`stop(context:completion:)`:
1. `task == nil` (beginSession never got a socket) → complete now with `.authFailed(msg)` if `failureIsAuth` else `.serverError(msg)`, default msg `"Deepgram is not configured."`;
2. install `completionGate = SafeCompletion { completion($0) }`, `waitingForFinalize = true`;
3. send text frame `{"type":"Finalize"}`;
4. arm a `DispatchWorkItem` at **+3.0 s** on the provider queue → `completeAndClose()`, stored in `finalizeWaitItem`.

Comment: *"server returns one last Results with `is_final=true` and `from_finalize=true`. We complete on receipt of that frame OR after a 3s timeout, whichever comes first."*

#### Response parsing

Switch on `json["type"] as? String`:

**`"Results"`** —
```swift
guard let isFinal = json["is_final"] as? Bool, isFinal else { return }   // interims ignored
guard let channel = json["channel"] as? [String: Any],
      let alternatives = channel["alternatives"] as? [[String: Any]],
      let first = alternatives.first,
      let transcript = first["transcript"] as? String,
      !transcript.trimmingCharacters(in: .whitespaces).isEmpty else {
    // Empty is_final frame after Finalize — still triggers close.
    if let from = json["from_finalize"] as? Bool, from, waitingForFinalize {
        finalizeWaitItem?.cancel(); finalizeWaitItem = nil; completeAndClose()
    }
    return
}
finalSegments.append(transcript)                       // under finalsLock
if detectedLanguage == nil {
    if let detected = channel["detected_language"] as? String { detectedLanguage = detected }
    else if let langs = channel["languages"] as? [String], let f = langs.first { detectedLanguage = f }
}
if let from = json["from_finalize"] as? Bool, from, waitingForFinalize {
    finalizeWaitItem?.cancel(); finalizeWaitItem = nil; completeAndClose()
}
```

JSON keys consumed, exhaustively: `type`, `is_final`, `from_finalize`, `channel.alternatives[0].transcript`, `channel.detected_language`, `channel.languages[0]`.

**`"Metadata"`** — explicitly ignored (`break`). Everything else — ignored.

#### `completeAndClose()`

```
guard let gate = completionGate, !gate.hasCompleted else { return }
waitingForFinalize = false; finalizeWaitItem = nil
task?.send(.string(#"{"type":"CloseStream"}"#)) { _ in }
queue.asyncAfter(deadline: .now() + 0.2) { self.tearDown(reason: "complete") }
collected = finalSegments.joined(separator: " ") ; finalSegments.removeAll()
cleaned = collected.trimmed
if cleaned.isEmpty {
    failureMessage != nil ? (failureIsAuth ? .authFailed(msg) : .serverError(msg)) : .emptyResult
} else {
    .success(TranscriptResult(id: UUID().uuidString, asrText: cleaned, formattedText: cleaned,
                              duration: packetCount * 0.04, numWords: cleaned.split(" ").count))
}
```

The 0.2 s delay: *"Give the server ~200ms to flush its close frame before we cancel. Tearing down too early can manifest as a spurious WS error in the receive loop."*

Success logs: `"Deepgram: detected_language=\(detected), got \(n) chars, \(%.1f)s"` when a language was detected, else `"Deepgram: got \(n) chars, \(%.1f)s"`.

#### Error mapping (`handleConnectionError`)

`statusCode = (task?.response as? HTTPURLResponse)?.statusCode ?? 0`. Early-out: `if task == nil && completionGate == nil { return }` (a post-delivery normal close is not an error). Then log `"Deepgram: WS error code=\(statusCode) — \(desc)"` and:

| Status | `failureMessage` (verbatim) | `failureIsAuth` |
|---|---|---|
| 401, 403 | `Deepgram: API key was rejected (HTTP <code>). Open Settings → Accounts → Deepgram and paste a fresh key from console.deepgram.com.` | `true` |
| 400, 404 | `Deepgram: bad request (HTTP <code>) — check model/language. <desc>` | `true` |
| 429 | `Deepgram: rate limited (HTTP 429). <desc>` | false |
| 500…599 | `Deepgram: server error HTTP <code>. <desc>` | false |
| default | `Deepgram: connection failed — <desc>` | false |

Then, if a gate exists: cancel the finalize wait, and fire `.authFailed(failureMessage)` if `failureIsAuth`, else `.connectionFailed` **if `statusCode == 0`** (no HTTP response at all), else `.serverError(failureMessage ?? "Deepgram error")`. Finally `tearDown(reason: "error")`.

`tearDown(reason:)`: stop keepalive, `task?.cancel(with: .normalClosure, reason: reason.data(using: .utf8))`, `task = nil`, `urlSession?.invalidateAndCancel()`, `urlSession = nil`, `isOpen = false`, drop buffered packets. The close reason string is one of `"cancel"`, `"error"`, `"complete"`.

Close-delegate log: `"Deepgram: stream closed code=\(closeCode.rawValue) reason=\(reasonStr)"` (verbose), `reasonStr` defaults to `"(none)"`.

#### Deepgram: Swift streaming vs. the Rust batch implementation — every difference

`crates/wl-providers/src/deepgram.rs` is a **batch** client against the pre-recorded HTTP endpoint. The differences are not cosmetic; nearly every axis differs.

| Axis | Swift (`8a81d74`) | Rust (`crates/wl-providers/src/deepgram.rs`) | Verdict |
|---|---|---|---|
| Transport | `wss://api.deepgram.com/v1/listen`, WebSocket | `POST https://api.deepgram.com/v1/listen`, `Content-Type: audio/wav` | **Must add streaming.** The batch path may stay as a second mode, but the trait must express streaming. |
| Body | raw s16le PCM binary frames, 1280 B each | one 44-byte-header WAV blob | Streaming sends headerless PCM; `encoding`/`sample_rate`/`channels` query params replace the header |
| Auth | `Authorization: Token <key>` header on upgrade | `Authorization: Token <key>` header | ✅ same |
| Key source | env `WISPR_LIGHTNING_DEEPGRAM_KEY` → `SecretsStore` (`~/Library/Application Support/WisprLightning/secrets/secrets.json`) | `CredentialStore` (OS keyring, account `deepgram-api-key`, service `com.wisprlightning.app`) with a 0600 `credentials.json` fallback | **Differs.** The Rust store is strictly better; keep it, but add the env-var override, which Rust lacks entirely |
| `model` | hard-coded `"nova-3"`, no UI to change it | `settings.deepgram_model`, defaulting to `"nova-3"`, with `is_nova3_family()` prefix logic | Rust is a superset; Swift's Accounts panel deliberately has no model picker (*"Deepgram's other models are either older or specialized for voice-agent turn detection"*). Keep the Rust flexibility, default to `nova-3`, don't add a picker |
| `encoding` | `linear16` | absent (WAV is self-describing) | Streaming **must** send it |
| `sample_rate` | `16000` | absent | Streaming **must** send it |
| `channels` | `1` | absent | Streaming **must** send it |
| `smart_format` | `true` | `true` | ✅ same |
| `punctuate` | `true` | `true` | ✅ same |
| `interim_results` | `false` | n/a (batch) | **Missing.** Explicitly disabling interims is what lets the parser treat every `Results` frame with `is_final` as a segment |
| `mip_opt_out` | `true` | **absent** | **Missing, and it is a privacy regression.** Rust currently opts the user's dictated audio into Deepgram's Model Improvement Program |
| Keyterm param | `keyterm` repeated, `phrases.prefix(50)` | `keyterm` repeated via `form_urlencoded`, `select_keyterms(vocab, KEYTERM_MAX_TOKENS=500)` | **Differs in the cap:** 50 *phrases* vs 500 *tokens*. Rust also filters terms containing `,`, `;` or a trailing `:<weight>`; Swift does not filter at all |
| Language: config | `settings.deepgramLanguage: String`, a Deepgram-only field, single value | `settings.languages: Vec<String>`, shared across vendors | **Differs.** The Swift model added a dedicated field precisely because *"auto-detect and multi-mode don't translate cleanly to other vendors"* |
| Language: auto | sentinel `"__auto__"` → `detect_language=true` | sentinel `"auto"` → `LanguageMode::Detect` → `detect_language=true` | **Sentinel string differs**: `"__auto__"` vs `"auto"` |
| Language: multi | sentinel `"__multi__"` → `language=multi`, explicit user choice | inferred: ≥2 configured languages → `language=multi` | **Differs.** Swift makes multi an explicit picker entry; Rust derives it |
| Language: codes | 35 plain BCP-47 codes, passed through verbatim | 104 Wispr-Flow codes, remapped by `deepgram_language_tag()` (`engb`→`en-GB`, `dech`→`de-CH`, `yue`→`zh-HK`, `zhcn`→`zh-Hans`, `zh`→`zh-Hant`, `hien`→`multi`) | **Differs.** Swift's list is already BCP-47, so no mapping is needed for the Deepgram picker. The Rust mapping stays relevant only if the shared `languages` list keeps feeding Deepgram |
| `detect_language` on streaming | used | Rust module doc explicitly notes *"`detect_language` is **not** supported on Deepgram's streaming endpoint … a streaming client would have to fall back to `multi`"* | **Direct contradiction to resolve.** Swift ships `detect_language=true` on the WS URL. Either the Rust note is wrong, or the Swift auto-detect mode silently does nothing on streaming. Do not silently drop the parameter — reproduce Swift's behaviour and log |
| Response shape | `{type, is_final, from_finalize, channel:{alternatives:[{transcript}], detected_language, languages}}` | `{metadata:{…}, results:{channels:[{alternatives:[{transcript}]}]}}` | **Entirely different.** The Rust `BatchResponse` doc already warns these must not be unified: *"streaming's is the slim `{request_id, model_info, model_uuid}` … the transcript sits under `results.channels[]` rather than a bare `channel`. One struct for both would silently accept the wrong payload"* |
| Segment assembly | `finalSegments.joined(separator: " ")` across every `is_final` frame | single `alternatives[0].transcript` | Streaming needs the accumulator |
| Keepalive | `{"type":"KeepAlive"}` every 5 s, skipped within 4.5 s of a send | n/a | **Missing** |
| Finalize | `{"type":"Finalize"}` + 3 s cap | n/a | **Missing** |
| Close | `{"type":"CloseStream"}` + 200 ms grace | n/a | **Missing** |
| Timeouts | 30 s upgrade; 3 s finalize; 0.2 s close grace | `TRANSCRIBE_TIMEOUT = 120 s`, `HEALTH_TIMEOUT = 10 s` | Batch timeouts don't map; keep 120 s only for a retained batch path |
| Health check | none in the provider; the Settings button hits `GET https://api.deepgram.com/v1/projects` (timeout 15) and reports `projects[0].name` | `GET /v1/auth/token` (timeout 10) | **Differs.** `/v1/auth/token` is the better check but returns no project name; Swift's UI surfaces `"Connected — project: <name>"` |
| Post-processing | none — `formattedText == asrText`, raw Deepgram output goes straight to the injector | `server_side_formatting: false` → local `postprocess` applies dictionary replacements/snippets | **Rust does more.** This is an intentional Rust improvement (the module doc explains it); keep it, but it means Rust and Swift produce different text for the same audio |
| Error mapping | 401/403 → authFailed; 400/404 → authFailed; 429 → serverError; 5xx → serverError; else connectionFailed/serverError by `statusCode == 0` | `ProviderError::from_status` with `QuotaExceeded` / `RateLimited` variants | Rust's split is finer. Map 429 → `RateLimited` (retryable), 400/404 → non-retryable-but-should-fallback to match Swift's chain behaviour |

### What the Rust port must change (providers, summary)

1. **Add three providers**: `wispr` exists; add `openrouter`, `claude_voice`, and a **streaming** Deepgram alongside (or replacing) the batch one.
2. `wl-providers` has no HTTP-JSON-chat provider at all — OpenRouter is entirely new: `reqwest` POST, 90 s timeout, the exact four headers, the inline-WAV message array, `classifyError` with all six status arms, `OpenRouterModels` fetch/filter/sort, and the `model_override` field on the provider struct.
3. Claude Voice is entirely new and **macOS-only** (the `Claude Code-credentials` Keychain item and the `claude` CLI are macOS artifacts). On Windows, `DictationVendor::ClaudeVoice` must still exist in the enum (settings compatibility) but `is_ready` should return false and `start()` must fail with a clear `NotConfigured`. `[INFERENCE]` — the Swift source is macOS-only and has no Windows story; this is the minimal coherent behaviour.
4. Every WS provider needs a real keepalive: Flow **20 s WS ping frame**, Claude Voice **8 s `{"type":"KeepAlive"}` text frame**, Deepgram **5 s `{"type":"KeepAlive"}` text frame with a 4.5 s skip window**. These are three different mechanisms and must not be unified.
5. Pre-open packet buffering is mandatory for both streaming providers. Without it the first ~1 s of every dictation is lost.

---

## 4. The fallback chain (B-012, commit `25b3315`)

### 4.1 Model

```swift
/// One step in the user-configured fallback chain. `vendor` is a
/// `DictationVendor` rawValue; `openRouterModel` is honoured only when
/// `vendor == openRouter` and lets the chain include multiple OpenRouter
/// models with different speed/quality tradeoffs.
struct FallbackStep: Codable, Hashable, Identifiable {
    var id: UUID
    var vendor: String
    var openRouterModel: String?

    init(vendor: String, openRouterModel: String? = nil) {
        self.id = UUID()
        self.vendor = vendor
        self.openRouterModel = openRouterModel
    }
}
```

`id` is generated in `init`, is `Codable`, and persists — it is the SwiftUI `ForEach` identity, so it must be stable across saves or rows lose focus mid-edit.

```swift
/// Ordered fallback chain. When the primary vendor fails with a hard
/// error (auth / connection / server / timeout), Lightning rebuilds the
/// dictation provider as `fallbackChain[0]`, retries with the same audio,
/// and walks the chain on subsequent failures.
var fallbackChain: [FallbackStep] = []
```

Default is **empty** — no fallback unless the user configures one.

### 4.2 Indexing

`AppDelegate.currentChainIndex: Int`, reset to 0 between dictations:

```
0     → settings.activeVendor  (the "primary")
1..N  → settings.fallbackChain[index - 1]
```

```swift
private func vendorAtChainStep(_ index: Int) -> DictationVendor {
    if index == 0 { return DictationVendor(rawValue: settings.activeVendor) ?? .wisprFlow }
    return DictationVendor(rawValue: settings.fallbackChain[index - 1].vendor) ?? .wisprFlow
}
private func activeVendorForChainStep() -> DictationVendor { vendorAtChainStep(currentChainIndex) }
private func hasNextChainStep() -> Bool { currentChainIndex < settings.fallbackChain.count }
```

The user-facing numbering is 1-based: the primary is displayed as `1.`, `fallbackChain[0]` as `2.` (`Text("\(index + 2).")`).

### 4.3 Advancement

```swift
private func advanceChainStep() -> DictationVendor {
    currentChainIndex += 1
    let vendor = vendorAtChainStep(currentChainIndex)
    dictationProvider.cancel()
    dictationProvider = providerForCurrentChainStep()
    dictationProvider.dictionaryStore = dictionaryStore
    return vendor
}

private func providerForCurrentChainStep() -> DictationProvider {
    let vendor = vendorAtChainStep(currentChainIndex)
    if currentChainIndex == 0 {
        return Self.makeProvider(vendor: vendor, session: session, settings: settings)
    }
    return Self.makeProvider(vendor: vendor, session: session, settings: settings,
                             openRouterModelOverride: settings.fallbackChain[currentChainIndex - 1].openRouterModel)
}

private static func makeProvider(vendor: DictationVendor, session: Session, settings: AppSettings,
                                 openRouterModelOverride: String? = nil) -> DictationProvider {
    switch vendor {
    case .wisprFlow:   return WisprFlowProvider(session: session, settings: settings)
    case .openRouter:  return OpenRouterProvider(settings: settings, modelOverride: openRouterModelOverride)
    case .claudeVoice: return ClaudeVoiceProvider(settings: settings)
    case .deepgram:    return DeepgramProvider(settings: settings)
    }
}
```

Note: **step 0 never gets a model override** — it uses `settings.openRouterModel`. Only steps ≥1 can carry a per-step model.

### 4.4 When a step is attempted

Inside `handleTranscriptionResult`'s `.failure` branch, **before** the auto-retry logic:

```swift
if error.shouldFallback && self.hasNextChainStep() {
    let nextVendor = self.advanceChainStep()
    wLog("Fallback: step \(self.currentChainIndex) → \(nextVendor.displayName) (after \(error.userMessage))")
    DispatchQueue.main.async {
        self.recordingOverlay.showRetrying(
            attempt: self.currentChainIndex + 1,
            maxAttempts: self.settings.fallbackChain.count + 1
        )
    }
    DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 0.3) { [weak self] in
        self?.attemptTranscription()
    }
    return
}
```

Precise semantics:

- **Delay between steps: 0.3 s** (vs. 1.5 s for a same-provider auto-retry).
- **Single-shot per step.** From the commit message: *"single-shot per step, no in-step retry … chain length IS the retry budget."* The chain branch `return`s before the `isRetryable` branch, so a chain step never gets the 2× auto-retry while more steps remain.
- **Counts as failure**: `.authFailed`, `.connectionFailed`, `.serverError`, `.timeout` (`shouldFallback == true`), *including* a `.timeout` synthesized by the per-provider watchdog.
- **Does not count**: `.emptyResult`. Commit message: *"emptyResult does NOT fall through (mic didn't catch speech, switching models won't help)."*
- **Same audio.** `pendingPackets` is untouched; `attemptTranscription()`'s re-prime block (`currentChainIndex > 0`) replays every packet into the new provider.
- **Chain exhausted** (`hasNextChainStep() == false`) → falls through to the normal path: 2× auto-retry if `isRetryable`, else the persistent retry UI.

### 4.5 The per-provider watchdog

```swift
private static let perProviderWatchdogBase: TimeInterval = 45
private static let perProviderWatchdogPerSecond: TimeInterval = 0.4
private static let perProviderWatchdogCap: TimeInterval = 300

let recordingSeconds = Double(packets.count) * 0.04
let watchdogSeconds = min(300, 45 + recordingSeconds * 0.4)
```

On fire: log `"Provider watchdog fired after \(Int(watchdogSeconds))s — forcing fallback"`, set `attemptWatchdogFired = true`, `dictationProvider.cancel()`, `gate.fire(.failure(.timeout))` — which then takes the chain branch. Both the watchdog and the provider's own completion funnel through one `SafeCompletion` gate, so advancement runs at most once.

Rationale for the scaling, verbatim: *"some backends (OpenRouter + Gemini doing audio-in-and-text-out, Wispr Flow on a long upload) take proportional time to a 10-minute recording — a flat 45s would pre-empt a legitimately slow result and falsely advance the chain."*

Separate and additional: `scheduleProcessingTimeout()` arms a main-thread `Timer` at `max(30.0, 30.0 + recordingDuration * 0.5)` that shows the persistent retry UI (message `"Timed out"`) without advancing the chain.

### 4.6 Reset points

- `clearPendingTranscription()` resets `currentChainIndex` (and `attemptStartedAt`) — called on success and on dismiss.
- `retryTranscription()` (the user pressing **Retry** on the error pill) restarts from the top: `currentChainIndex = 0`, `currentRetryAttempt = 1`, cancel + rebuild the **primary** provider, `showProcessing()`, `prewarmConnection()`, `scheduleProcessingTimeout()`, then `attemptTranscription()` on a background queue. Comment: *"Manual retry restarts the chain from the top … the user gets a fresh 2x auto-retry budget."*

### 4.7 What the user sees

- **During a hop**: `recordingOverlay.showRetrying(attempt: currentChainIndex + 1, maxAttempts: fallbackChain.count + 1)` — the yellow "Retrying" pill state, e.g. `2/3` when hopping to the first fallback of a two-step chain.
- **Log line**: `Fallback: step 1 → OpenRouter (after Connection failed — check your network)`.
- **Telemetry**: `AttemptRecord` records `fallbackHops = currentChainIndex` and `finalVendor = activeVendorForChainStep().displayName` (nil on failure), surfaced in the status-bar "Recent dictations" submenu.
- **Settings → Provider** header copy, verbatim:
  > Step 1 is your primary provider. If it fails with a hard error (auth, network, server, timeout), Lightning automatically retries the same audio against step 2, then step 3, and so on. Empty transcripts don't fall through. Set up vendor credentials in the Accounts tab.

### 4.8 Configuration and reordering (Settings → Provider)

Row 1 (`primaryRow`) is bound to `settings.activeVendor` + `settings.openRouterModel`; rows 2..N+1 are `FallbackStepRow`s bound to `fallbackChain[i]`. Each row: `Picker` over `DictationVendor.allCases` (max width 280), a `VendorReadinessBadge`, and — only when the row's vendor is `openrouter` — a model `Picker` (max width 420) fed by the shared live model list.

Mutations, verbatim behaviour:

| Action | Effect |
|---|---|
| `+ Add fallback` | `existingVendors = Set(chain.map(vendor)) ∪ {activeVendor}`; append `FallbackStep(vendor: DictationVendor.allCases.first { !existingVendors.contains($0.rawValue) } ?? .openRouter)`. So the default new step is the first unused vendor in declaration order, falling back to OpenRouter when all four are used |
| Remove (`⊗`) | `NSAlert`: messageText `"Remove this fallback step?"`, informativeText `"Step \(index + 2) (\(displayName)) will be removed from the chain."`, buttons `Remove` / `Cancel`. Only on `.alertFirstButtonReturn` does `removeFallbackStep(at:)` run |
| Move up, `index > 0` | `moveFallbackStep(from: index, to: index - 1)` |
| Move up, `index == 0` | `promoteToPrimary(at: 0)` — swaps the row with the primary |
| Move down | shown only when `index < chain.count - 1`; calls `moveFallbackStep(from: index, to: index + 2)` |
| Primary `chevron.down` | `demotePrimary()`. Tooltip: `"Move primary down (appends a new fallback step)"` when the chain is empty, else `"Move primary down — swap with the first fallback"` |

```swift
func moveFallbackStep(from src: Int, to dst: Int) {
    guard fallbackChain.indices.contains(src), dst >= 0, dst <= fallbackChain.count, src != dst else { return }
    let step = fallbackChain.remove(at: src)
    let insertAt = dst > src ? dst - 1 : dst
    fallbackChain.insert(step, at: insertAt)
    saveFallbackChain()
}
```

The `dst > src ? dst - 1 : dst` adjustment is why "move down" passes `index + 2` rather than `index + 1`: the destination is expressed in pre-removal coordinates.

`promoteToPrimary(at:)` and `demotePrimary()` both carry the OpenRouter model override across the swap, but **only when the vendor actually is OpenRouter**:

```swift
let demoted = FallbackStep(
    vendor: activeVendor,
    openRouterModel: activeVendor == DictationVendor.openRouter.rawValue ? openRouterModel : nil
)
```
*"Carry its OpenRouter model override only if it actually was OpenRouter — otherwise the field is meaningless and would leak into a different vendor's slot."*

`demotePrimary()` with an empty chain picks `DictationVendor.allCases.first { $0.rawValue != activeVendor } ?? .openRouter` as the new primary and appends the old one.

`updateFallbackStepVendor(at:vendor:)` **nils `openRouterModel`** whenever the new vendor isn't OpenRouter. `updateFallbackStepModel(at:model:)` trims and maps empty → `nil`.

Every mutation calls `settings.save()` immediately (no explicit Save button in this panel).

### What the Rust port must change
There is no chain at all in the Rust port — `wl_providers::build(id, settings, session)` constructs exactly one provider from `settings.provider`.

- Add `FallbackStep { id: Uuid, vendor: ProviderId, openrouter_model: Option<String> }` and `Settings::fallback_chain: Vec<FallbackStep>`, serialized under the JSON key `fallbackChain` with fields `id` / `vendor` / `openRouterModel` (camelCase, matching Swift's default `Codable` encoding) so `settings.json` round-trips.
- Move chain orchestration into the pipeline crate: `current_chain_index`, `vendor_at_chain_step`, `has_next_chain_step`, `advance_chain_step`, and the re-prime loop. The pipeline must retain the full packet buffer for the whole attempt sequence.
- Implement `should_fallback` on `ProviderError` (§1.4) and branch on it **before** the retryable/auto-retry branch, with a **0.3 s** inter-step delay vs. **1.5 s** for in-place retry.
- Port the watchdog: `min(300, 45 + recording_secs * 0.4)`, plus the separate processing timeout `max(30, 30 + recording_secs * 0.5)`.
- `build()` needs the `openrouter_model_override` parameter, applied only for steps ≥ 1.
- Overlay: `show_retrying(attempt: idx + 1, max_attempts: chain.len() + 1)`.
- Settings UI: the Provider tab with numbered rows, add/remove/reorder including promote/demote across the primary boundary, and the per-step OpenRouter model picker fed by one shared model fetch.

---

## 5. Polish gating

### 5.1 The predicate

```swift
/// True when the user is signed in via the Wispr Flow OAuth flow.
/// Polish, AI formatting, and the Wispr Flow backend all require this.
/// Other vendors (OpenRouter, Claude Voice) authenticate independently.
var isWisprFlowAccount: Bool { return isValid }

/// True when polish features should be exposed. Requires both a valid Flow
/// session and the active vendor being Wispr Flow — when the user is on
/// OpenRouter / Claude Voice we don't surface polish even if they still
/// have a Flow session cached.
func canUsePolish(activeVendor: DictationVendor) -> Bool {
    return activeVendor == .wisprFlow && isWisprFlowAccount
}
```

So: **`activeVendor == .wisprFlow` AND `session.isValid`** (a token expiring within 60 s already fails). Both conditions, always.

Crucially the argument is the **primary** vendor (`settings.activeVendor` / `AppDelegate.activeVendor`), **not** `activeVendorForChainStep()`. A chain that falls back to Flow mid-dictation does **not** enable polish, and a chain whose primary is Flow keeps polish visible even after it falls back to Deepgram.

### 5.2 Every gated surface

There are exactly **five** call sites.

| # | Site | Effect when `false` |
|---|---|---|
| 1 | `UI/SettingsWindow.swift:149` — `AllSettingsView.settingsGroup` | The sidebar list is `[.general, .dictation, .accounts, .provider, .polish]` when true, `[.general, .dictation, .accounts, .provider]` when false. **The entire Polish tab disappears from the sidebar.** Comment: *"Polish is a Wispr Flow-only feature; hide the tab entirely for other vendors."* The vendor is read from `vm.activeVendor`, so switching the picker re-evaluates live |
| 2 | `Services/HotkeyListener.swift:45` — `rebuildHotkeySet()` | `_polishKeyCodes = canUsePolish ? Set(settings.polishHotkeyKeyCodes) : []`. **The polish hotkey (default keycode 62, Right Control) is not registered at all** — the key falls through to the focused app. `rebuildHotkeySet()` re-runs on both `.settingsChanged` and `.sessionChanged` (main queue), so signing out or switching vendors unbinds it without a relaunch |
| 3 | `App/AppDelegate.swift:820` | Guards the branch that *skips* the immediate inject in favour of auto-polish; when false, the normal `showInserting()` + `inject` path runs and the audio artifact is deleted on success |
| 4 | `App/AppDelegate.swift:862` | Guards the actual `self.autoPolishText(displayText)` call |
| 5 | `App/AppDelegate.swift:1229` | `guard session.canUsePolish(activeVendor: activeVendor) else { … }` — the manual polish-hotkey handler bails |

Sites 3 and 4 are both `canUsePolish(...) && settings.autoPolish && settings.polishEnabled`; site 3 additionally requires `!activeInstructions.isEmpty`.

`AppSettings` keeps every polish field regardless (`polishEnabled`, `polishInstructions`, `autoPolish`, `polishHotkeyKeyCodes = [62]`, `polishHotkeyLabels = ["Right Control"]`). B-007 scope, verbatim: *"keep `PolishService` / `PolishStore` / polish hotkey code intact, but hide all polish UI … Settings model: leave fields, hide UI."* Nothing is deleted or reset when the gate closes; flip back to Flow and the user's polish configuration is exactly as they left it.

Default polish instructions (unchanged, for completeness):
```
"Make more concise": true          "Reword for clarity": true
"Maintain your tone": true         "Reorder for readability": true
"Add structure for readability": true
"Clarify main point": false        "Refine phrasing for impact": false
```
`activePolishInstructions` = the keys whose value is `true`.

### What the Rust port must change
- Add `Session::can_use_polish(active_vendor: ProviderId) -> bool` = `active_vendor == ProviderId::WisprFlow && session.is_valid()`, with `is_valid()` including the **60-second** expiry margin.
- Gate the **Polish settings route** out of the frontend nav entirely (not disabled — absent) when the predicate is false, recomputed reactively on both vendor change and session change.
- Skip polish hotkey registration in `wl-platform` when false, and re-register on both settings-changed and session-changed events.
- Gate the auto-polish branch and the manual polish hotkey handler.
- Pass the **primary** vendor, never the current chain step.
- Persist all polish settings unchanged while gated.

---

## 6. Secrets

Two stores coexist, with a one-way migration between them.

### 6.1 `SecretsStore` — file-backed, the current home for everything

```
Path: ~/Library/Application Support/WisprLightning/secrets/secrets.json
Directory mode: 0700 (created with, and re-applied on every access)
File mode: 0600 (baked into FileManager.createFile attributes)
Format: JSON object of String → String, pretty-printed
```

```swift
enum Key: String {
    case openRouterAPIKey
    case claudeCodeTokenMirror
    case deepgramAPIKey
}
```

Raw values are the case names verbatim: `"openRouterAPIKey"`, `"claudeCodeTokenMirror"`, `"deepgramAPIKey"`.

API: `read(_:) -> String?` (empty string reads as `nil`), `write(_:_:) -> Bool` (nil/empty value deletes the key), `delete(_:) -> Bool` (= `write(key, nil)`), `has(_:) -> Bool`.

Migration built into the `fileURL` lazy initializer: if `…/WisprLightning/secrets.json` exists and `…/WisprLightning/secrets/secrets.json` does not, the legacy file is **moved**. Rationale for the subdirectory: *"even if a future code path writes a more-permissive sibling file, this directory is strictly owner-only and the file inside it inherits that protection."*

`persist(_:)` is deliberately not `Data.write(options: .atomic)`:

> Create the file with 0600 from the start so the contents are never briefly world-readable. `Data.write(options:.atomic)` would use the user's default umask (usually 022 → 644) and a follow-up chmod is racy and silently fails.

It removes any existing file, then `FileManager.createFile(atPath:contents:attributes: [.posixPermissions: 0o600])`. Failure logs `"SecretsStore: failed to write secrets file at \(path)"` and returns `false`.

`write` **persists before updating the in-process cache**, and restores the previous cache on failure: *"if the disk write fails we don't want the in-process cache to diverge from what the next launch's `loadIfNeeded()` will read off disk."*

`loadIfNeeded()` holds the lock across the file read (*"Two threads racing here would otherwise each parse the same file independently"*). The cache is loaded once per launch and never invalidated by an external file change.

### 6.2 `KeychainStore` — legacy, OpenRouter only

```
Service:        "com.wisprlightning"
Legacy service: "com.wispr.edge"          (from the retired wispr-edge app)
Class:          kSecClassGenericPassword
Accessibility:  kSecAttrAccessibleAfterFirstUnlock
enum Key: String { case openRouterAPIKey = "openrouter.api_key" }
```

**One key only.** There has never been a Keychain entry for Deepgram or for the Claude Code mirror. Reads are cached in a process-wide `[Key: String]` under `cacheLock`. `read` also migrates from `com.wispr.edge` on first hit, and **only** then rewrites.

### 6.3 `hasOpenRouterKeyHint()` — the prompt-free probe

```swift
let query: [String: Any] = [
    kSecClass:            kSecClassGenericPassword,
    kSecAttrService:      "com.wisprlightning",
    kSecAttrAccount:      "openrouter.api_key",
    kSecMatchLimit:       kSecMatchLimitOne,
    kSecReturnAttributes: true,
    kSecReturnData:       false,
]
return SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess
```

> macOS returns errSecSuccess without firing the access-control dialog because we're not asking for the data, just the catalog entry.

### 6.4 Where each secret lives, and why

| Secret | Store | Path / item | Written by |
|---|---|---|---|
| Wispr Flow Supabase session | plaintext JSON file | `~/Library/Application Support/WisprLightning/session.json` (migrated once from `~/Library/Application Support/Wispr Flow/session.json`) | Lightning |
| OpenRouter API key | `SecretsStore` | `secrets/secrets.json` key `openRouterAPIKey` | Lightning (migrated out of Keychain on first use) |
| Deepgram API key | `SecretsStore` | `secrets/secrets.json` key `deepgramAPIKey` | Lightning |
| Claude Code OAuth token | Keychain, **read-only** | service `Claude Code-credentials`, no account attr | the `claude` CLI |
| Claude Code token **mirror** | `SecretsStore` | `secrets/secrets.json` key `claudeCodeTokenMirror` (JSON-encoded `ClaudeCodeOAuthToken`) | Lightning |
| OpenRouter API key (legacy) | `KeychainStore` | service `com.wisprlightning`, account `openrouter.api_key` | prior builds; deleted after migration |
| OpenRouter API key (Edge legacy) | `KeychainStore` | service `com.wispr.edge`, account `openrouter.api_key` | the retired `wispr-edge` app |
| Claude Code mirror (legacy) | Keychain | service `com.wisprlightning`, account `claude_code.cached_token` | prior builds; deleted on first read |

### 6.5 Why the key moved OUT of the Keychain — the reasoning, since it looks backwards

Moving a credential from the OS secret store to a plaintext file reads like a security regression. It is a deliberate, documented tradeoff, arrived at over four commits.

**The failure mode.** macOS Keychain ACLs are keyed on **code signing identity**, specifically the binary's cdhash. Lightning is built locally with `swift build -c release` and packaged by `build-app.sh`; every rebuild changes the cdhash. To macOS, each rebuild is a *different application* asking to read an item some *other* application created — so it prompts for the login password. Every launch. Sometimes several times per launch.

**The four commits, in order:**

1. `5396d00` *"KeychainStore: cache reads + rewrite-on-first-read to stop repeat prompts"* — added the process-wide cache (one prompt per launch instead of one per read) and, on the first successful read, rewrote the item under the current identity so *"future launches' first read should then be silent."*
2. `37b36ac` *"Stop the keychain prompt loop on every Settings open"* — the rewrite in (1) was itself the bug. `SecItemDelete + SecItemAdd` creates a **brand-new item with no "Always Allow" history**, so the next launch prompted again, rewrote again, and looped forever. The rewrite was narrowed to the legacy-service migration only. The current comment says so verbatim:
   > Only rewrite when migrating from the legacy service. Rewriting on every read causes the keychain-prompt loop: each `SecItemDelete` + `SecItemAdd` creates a brand-new keychain item with no "Always Allow" history, so the next launch prompts again, we rewrite again, and the loop continues forever.

   The same commit also deferred the read out of `SettingsViewModel.init` into the OpenRouter panel's on-demand loader, so *"opening Settings to change a hotkey or pick a vendor now touches zero keychain items."*
3. `c0094c8` *"Move OpenRouter key to plaintext file — no more Keychain prompts"* — even with the loop fixed, opening the Accounts tab still prompted, because the item had been written by a prior code identity and the ACL was already poisoned. There is no in-app remedy for that; the only fix is to stop using the Keychain. Commit message:
   > Cross-code-identity Keychain ACLs are the root cause … SecretsStore is a new file-backed store at `~/Library/Application Support/WisprLightning/secrets.json` (mode 600), which never triggers Keychain prompts. **Same security posture as the existing session.json sitting next to it.**
4. `87b24c1` *"/simplify SecretsStore"* — hardened the result: conditional Keychain delete (a failed file write no longer wipes the key from both stores), 0600 baked into `createFile` instead of a racy post-hoc chmod that *"silently failed, leaving the OpenRouter API key world-readable"*, and persist-before-cache-update.

**Why it isn't actually a regression.** The Supabase **refresh token** — a credential that mints new access tokens indefinitely — was already sitting in a plaintext `session.json` in the same directory. Adding a BYO OpenRouter key beside it does not change the threat model: any process running as this user could already read the strictly more valuable credential. Meanwhile the Keychain was, in practice, delivering *zero* additional protection while imposing a password prompt on every interaction. `SecretsStore.swift`'s own doc says: *"Storing in Application Support is plaintext but only readable by the current user, and matches how Lightning already stores its Supabase session file (Session.swift). The user explicitly preferred this tradeoff for the BYO OpenRouter key (no prompts) over Keychain."*

**What stayed in the Keychain, and why.** The `Claude Code-credentials` item is owned by the `claude` CLI. Lightning cannot move it — moving it would break the CLI. So Lightning reads it **once**, mirrors the decoded token into `SecretsStore` under `claudeCodeTokenMirror`, and reads the mirror from then on. The cross-app prompt fires at most once per token lifetime, and only on a user-initiated action. `c0094c8`: *"Claude Voice's cross-app Keychain read still prompts on the explicit 'Check' button — that's deliberate and gated on user action."* This is also why the Settings panel uses an explicit **Check** button rather than probing on view-appear:

> Deliberately not part of `PermissionStatusPoller` because the first Keychain read after a fresh launch triggers a macOS password dialog — we let the user fire it explicitly with the "Check" button instead of at view-appear time.

`ClaudeVoiceAuthCheck.check()` calls `ClaudeCodeKeychain.clearAllCaches()` then `read(forceRefresh: true)` (so a fresh `claude /login` is picked up), maps the result to `.signedIn` / `.expired` / `.notSignedIn`, and then calls `NSApp.activate(ignoringOtherApps: true)` because *"The Keychain password dialog steals focus away from us."*

### 6.6 Accounts tab (Settings → Accounts)

One card per vendor, in `DictationVendor.allCases` order. Header copy: *"Set up sign-in or API keys for each vendor here. Use the Provider tab to choose which one is active and arrange the fallback chain."*

| Card | Contents |
|---|---|
| **Wispr Flow** | Copy: *"Sign in with your Wispr Flow account to use Flow's WebSocket transcription pipeline. Auth is shared with the official Wispr Flow desktop app via a Supabase session file."* Signed-in: avatar + name + email + `Sign Out`. Signed-out: `Sign In with Google` → `AuthService.signInWithBrowser()` |
| **OpenRouter** | Copy: *"BYO key. You pay OpenRouter directly. Get a key at openrouter.ai/keys."* `Label("Saved", systemImage: "checkmark.seal.fill")` in green when `SecretsStore.has(.openRouterAPIKey)`. Secure field placeholder `"sk-or-… (paste to replace, leave empty to keep saved)"`, monospaced, with an eye toggle that lazily calls `loadOpenRouterAPIKeyIfNeeded()`. `Save` (disabled when the field is blank) → `"Saved."` or `"Save failed — couldn't write to secrets.json."`. `Test connection` → `GET https://openrouter.ai/api/v1/auth/key`, `Authorization: Bearer <key>`, timeout 15; reads `json["data"]["label"]`, `["limit"]`, `["usage"]`; message `"Connected — key label: \(label)"` plus `"; usage $X / $Y"` when `limit` is present; failures: `"No API key saved or entered"`, `"HTTP \(code)"`, `"Malformed response"` |
| **Claude Voice** | Copy: *"Sends audio live to Claude Code's STT WebSocket. Auth uses the OAuth token the `claude` CLI stores in your Keychain — Wispr Lightning never writes to it."* When `!ClaudeCodeKeychain.isCLIInstalled`, an info row: *"Claude CLI not detected"* / *"Lightning's Claude Voice provider needs the `claude` CLI. Install it from claude.ai/download, then run `claude /login` to sign in."* + `Open download page` → `https://claude.ai/download`. Buttons by state: `.unchecked` → `Check`; `.checking` → spinner; `.signedIn` → green `Signed in`; `.expired`/`.notSignedIn` → `Copy command` (copies literally `claude /login` to the pasteboard) + `Re-check`. Icons: `checkmark.circle.fill` green / `questionmark.circle.fill` secondary / `exclamationmark.circle.fill` orange |
| **Deepgram** | Copy: *"BYO key. You pay Deepgram directly ($0.0048/min for Nova-3, $200 free credit). Get a key at console.deepgram.com."* Same Saved badge / secure field / eye / Save shape as OpenRouter, placeholder `"API key (paste to replace, leave empty to keep saved)"`. `Test connection` → `GET https://api.deepgram.com/v1/projects`, `Authorization: Token <key>`, timeout 15; reads `json["projects"][0]["name"]`; message `"Connected."` or `"Connected — project: \(name)"`; 401/403 → `"API key rejected"`, other non-2xx → `"HTTP \(code)"`. Below it, a Language `Picker` (max width 320): `Auto-detect` (`__auto__`), `Multilingual (code-switching)` (`__multi__`), a `Divider()`, then the 35 entries as `"\(name) (\(code))"` |

Both key fields treat **empty input as "keep the existing value"**, never as delete: `saveOpenRouterAPIKey`/`saveDeepgramAPIKey` `guard !trimmed.isEmpty else { return false }`. Both Test buttons prefer the freshly-typed value and fall back to the saved one.

### What the Rust port must change
- `wl-providers::credentials::CredentialStore` currently knows two accounts: `wispr-session` and `deepgram-api-key`, service `com.wisprlightning.app`. Add `openrouter-api-key` and `claude-code-token-mirror`.
- **Keep the OS keyring as the primary store.** The Swift retreat to a plaintext file was forced by locally-signed-rebuild cdhash churn on macOS, which does not apply to a properly signed shipping app and does not apply at all to Windows Credential Manager. The Rust `CredentialStore` already has the right shape — keyring first, 0600 file fallback, one-way per-process degradation, warn-once.
- **But add the env-var overrides**, which Rust lacks: `WISPR_LIGHTNING_OPENROUTER_KEY` and `WISPR_LIGHTNING_DEEPGRAM_KEY` must be checked **before** the store, and `VOICE_STREAM_BASE_URL` must override the Claude Voice base URL.
- Add a `has(account) -> bool` that does not read the value, for the "Saved" badge.
- Add the read-only `Claude Code-credentials` reader (macOS `security`/`SecItemCopyMatching`, service attribute only, no account), the `claudeAiOauth` envelope with **millisecond** `expiresAt` and **no skew margin**, the mirror-to-credential-store cascade, and `clear_all_caches()` for the Re-check button.
- Port the migration semantics: only delete the old copy after the new write **succeeds**.
- Port the Accounts UI: four cards, per-vendor test-connection endpoints exactly as listed, empty-input-means-keep, prompt-free Saved badges, and the Claude Voice explicit-Check button (never probe on mount).

---

## 7. `KeyTerms` — NLTagger vocabulary boost

### 7.1 `ClaudeVoiceKeyTerms`

Module doc, verbatim — this is the reason the type exists:

> Wispr Flow's transcription pipeline takes a full OCR context blob and feeds it to an LLM formatter. The Claude Code STT endpoint can only consume Deepgram's `keyterms` vocabulary boost (a list of strings in the WS URL), so we distil OCR / dictionary lines down to high-signal nouns, drop UI noise / stopwords / short tokens / numerics, dedupe, and return the top-N by frequency.

Algorithm (`extract(from lines: [String], limit: Int = 20) -> [String]`):

1. `guard !lines.isEmpty else { return [] }`.
2. One `NLTagger(tagSchemes: [.lexicalClass])`, reused across all lines. Options: `[.omitPunctuation, .omitWhitespace, .joinNames]`. Unit `.word`, scheme `.lexicalClass`.
3. Keep only tokens whose tag is one of `.noun`, `.placeName`, `.personalName`, `.organizationName`.
4. `clean(token)`, then `counts[cleaned, default: 0] += 1`.
5. Rank by descending count, ties broken by **ascending key** (`l.key < r.key`), take `prefix(limit)`.

`clean(_:)` rejects a token unless all hold:
- after `.trimmingCharacters(in: .punctuationCharacters)` then `.trimmingCharacters(in: .whitespaces)`, **`count >= 4`**;
- not numeric — `isNumeric` is `s.unicodeScalars.allSatisfy { CharacterSet.decimalDigits.contains($0) || $0 == "." || $0 == "," }`;
- `lowercased()` not in `stopwords`.

The kept string retains its **original case** (only the stopword test is lowercased).

The stopword set, verbatim and complete (**65 entries**):

```
the, and, for, with, from, this, that, these, those,
have, has, had, are, was, were, been, being, but, not,
you, your, they, their, them, him, her, his,
what, when, where, which, who, why, how,
can, will, would, could, should, may, might, must,
ok, okay, yes, no, cancel, settings, edit, view, file,
open, close, save, delete, new, home, back, next,
search, send, reply, type, click, tap
```

Note the second half is UI chrome, not English stopwords — these are the words OCR of a normal screen returns constantly. `"no"`, `"ok"`, `"new"`, `"tap"` are all shorter than 4 characters and are therefore already rejected by the length gate; they are in the set defensively.

### 7.2 How OCR/AX context becomes keyterms — the one-recording-behind pipeline

The URL is fixed at connect time, and connect happens in `start()` — before this recording's OCR has finished (OCR runs in parallel with recording on `ocrQueue`). Verbatim from `ClaudeVoiceProvider`:

> OCR / screen-context lines for the *next* session. Set by AppDelegate from whatever OCR finished during the previous recording — the WS URL fixes keyterms at connect-time, so we can't add them retroactively. First recording of the launch sees an empty list; subsequent recordings get keyterms distilled from the preceding session's screen capture.

The wiring (commit `f272742`):

- `AppDelegate.lastSessionOcrLines: [String]`, populated at the **end** of each recording:
  ```swift
  self.pendingOcrContext = self.ocrQueue.sync { let ctx = self.cachedOCRContext; self.cachedOCRContext = []; return ctx }
  if let ocr = self.pendingOcrContext, !ocr.isEmpty { self.lastSessionOcrLines = ocr }
  ```
- At the **start** of the next recording, before `dictationProvider.start()`:
  ```swift
  if let cv = dictationProvider as? ClaudeVoiceProvider {
      cv.setPendingOcrLines(lastSessionOcrLines)
  }
  dictationProvider.start()
  ```
  `setPendingOcrLines` writes `pendingOcrLines` under `hintLock: NSLock`.

Merge in `beginSession()`:

```swift
var keyterms = ClaudeVoiceKeyTerms.extract(from: ocrLines, limit: 20)
if let phrases = dictionaryStore?.getVocabularyPhrases() {
    for phrase in phrases where !keyterms.contains(phrase) {
        keyterms.append(phrase)
        if keyterms.count >= 20 { break }
    }
}
```

Verbatim rationale: *"Dictionary phrases bypass the NL tagger — they're already curated proper nouns — and are appended to whatever the tagger distills from OCR."*

Consequences worth stating explicitly:
- **OCR wins over the dictionary.** The tagger's 20 slots are filled first; dictionary phrases only fill what's left. With a screen full of nouns, none of the user's dictionary reaches Claude Voice.
- The dedup is `!keyterms.contains(phrase)` — exact, case-sensitive `String` equality.
- The cap is 20 total (`limit: 20` and `keyterms.count >= 20`).
- **`axContext` is never converted to keyterms.** Only OCR lines and dictionary phrases. Per `CLAUDE.md`, AX context is empty in practice anyway (B-002 wontfix).

### 7.3 Per-vendor vocabulary shapes — the full matrix

| Vendor | Shape | Source | Cap | NLTagger? |
|---|---|---|---|---|
| Wispr Flow | full context blob: `dictionary_context` (phrases array), `dictionary_replacements` (map), `dictionary_snippets` (map of single-element arrays), plus `ocr_context` and `ax_context` as separate arrays | dictionary + live OCR + live AX, all from `stop(context:)` | none | no |
| OpenRouter | a sentence appended to the system prompt: `"\n\nThe speaker frequently uses these proper nouns or jargon — spell them as written: <joined>."` | dictionary phrases only | `prefix(40)` | no |
| Claude Voice | repeated `keyterms=` URL query params, fixed at connect | NLTagger over **previous** recording's OCR, then dictionary phrases | 20 total | **yes** |
| Deepgram | repeated `keyterm=` URL query params, fixed at connect | dictionary phrases only | `prefix(50)` | no |

Four vendors, four different vocabulary encodings, three different caps, and only one of them runs the tagger. There is no shared abstraction in the Swift code and attempting one would lose information in three directions.

### What the Rust port must change
- Port `ClaudeVoiceKeyTerms` — but macOS `NLTagger` has no Rust binding and no Windows equivalent. Options, in preference order:
  1. FFI to `NaturalLanguage.framework` on macOS with a POS-tagger crate or a heuristic fallback elsewhere — matches Swift exactly on the platform that has Claude Voice, and Claude Voice is macOS-only anyway (§3, item 3), so **the fallback path is never exercised for this vendor**.
  2. A pure-Rust heuristic: capitalized-token + length + stopword filter. Cheaper, and measurably different output.
  Take option 1. Everything downstream of the tagger — the 65-word stopword set, `>= 4` chars, the numeric test including `.` and `,`, count-desc / key-asc ranking, `limit: 20` — is trivially portable and must match literally.
- Wire the one-recording-behind OCR hint: `last_session_ocr_lines` on the pipeline, handed to the provider **before** `start()`.
- Do not unify the four vocabulary encodings. `VocabSupport` in `wl-providers` currently has `Full`, `Keyterm { max_tokens }`, `None` — it needs a fourth shape for OpenRouter's prompt-injected list, and `Keyterm`'s budget must be expressible in *phrases* (Claude Voice 20, Deepgram 50) as well as tokens.

---

## What the Rust port must change — consolidated

### A. Trait: batch → streaming (blocking, do first)

`TranscriptionProvider::transcribe(&self, req: &TranscribeRequest)` takes the whole recording up front. Claude Voice cannot be implemented behind it at all, and Deepgram-as-shipped cannot either. Replace with a session-oriented trait:

```rust
#[async_trait]
pub trait TranscriptionProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn prewarm(&self) {}
    fn cancel_prewarm(&self) {}
    async fn health(&self) -> Result<()>;
    /// Opens the vendor session. Streaming vendors connect here.
    fn start(&self) -> Result<Box<dyn DictationSession>>;
}

pub trait DictationSession: Send {
    /// Called from the audio callback — must never block or allocate on the hot path.
    fn feed(&self, packet: &[i16]);
    /// Finalize and deliver. Fires exactly once.
    fn stop(self: Box<Self>, ctx: DictationContext) -> BoxFuture<'static, Result<TranscriptResult>>;
    fn cancel(self: Box<Self>);
}
```

`DictationContext { app: AppContext, ocr_context: Vec<String>, ax_context: Vec<String>, dictionary: DictionaryContext, languages: Vec<String>, transcript_id: String }`. `packets` leaves the request type. `ProviderCapabilities` gains `streaming: bool` and `context_at_start: bool`.

### B. Vendors

- `ProviderId`: add `OpenRouter`, `ClaudeVoice`; re-key `Wispr` → serialized `"wispr_flow"` (with a legacy `"wispr"` migration); display names exactly `Wispr Flow` / `OpenRouter` / `Claude Voice` / `Deepgram`; iteration order Flow, OpenRouter, Claude Voice, Deepgram.
- Add `ProviderId::is_ready(&Session) -> bool` with the four arms of §2.2, including Claude Voice's unconditional `true`.
- **OpenRouter** (new): `POST https://openrouter.ai/api/v1/chat/completions`, headers `Authorization: Bearer`, `Content-Type: application/json`, `X-Title: Wispr Lightning`, `HTTP-Referer: https://github.com/cefege/wispr`, timeout 90 s, body per §3.2 with `"stream": false` and the `input_audio`/`wav` content part, `model_override` field, default `google/gemini-2.5-flash-lite`, the six-arm `classify_error`, the 64 KiB body-parse cap and the HTML-detection guard on the fallback message, the 40-phrase dictionary sentence, `asr_text == formatted_text`.
- **OpenRouter model picker** (new): `GET https://openrouter.ai/api/v1/models`, timeout 20 s, no auth; filter `architecture.input_modalities contains "audio"`; prices are strings-or-doubles, `""`/`"-"` → none, scaled ×1e6; sort by prompt price then id; label `"<name> — <in> / <out>"` with `"free"` for ≤0 and `$%.2f` otherwise; idempotent per session; "Custom: <id>" row for an off-list selection.
- **Claude Voice** (new, macOS-only): `wss://api.anthropic.com/api/ws/speech_to_text/voice_stream` with the eight fixed query params plus repeated `keyterms`; the four headers including `anthropic-client-platform: claude_code_cli`; 8 s `{"type":"KeepAlive"}`; binary 1280-byte PCM frames; pre-open buffering; `{"type":"CloseStream"}` + 2 s finalize cap; the four message types; interim-promoted-to-final semantics; `Claude Code-credentials` read-only Keychain access with the `SecretsStore` mirror; the auth-substring heuristic; `.authFailed(None)` + cache clear on auth failure.
- **Deepgram**: add the streaming path per §3.4 and the difference table — `wss://api.deepgram.com/v1/listen`, `Authorization: Token`, the eight query params **including `mip_opt_out=true` and `interim_results=false`** (both currently missing, the former a privacy regression), repeated `keyterm` capped at 50 phrases, 5 s/4.5 s keepalive, `{"type":"Finalize"}` + 3 s, `{"type":"CloseStream"}` + 0.2 s, the streaming response shape (`channel.alternatives[0].transcript`, `is_final`, `from_finalize`, `detected_language`, `languages[0]`), segment accumulation joined by `" "`. Add `settings.deepgram_language: String` with sentinels `"__auto__"` / `"__multi__"` and the 35-entry BCP-47 list — note the Rust sentinel is currently `"auto"` and multi is *inferred* from list length, both of which differ. Resolve the `detect_language`-on-streaming contradiction (§3.4) rather than silently dropping the param.

### C. Fallback chain

New end to end. `FallbackStep { id, vendor, openrouter_model }` serialized as `fallbackChain` with camelCase fields; `current_chain_index` in the pipeline; `should_fallback` on `ProviderError`; branch before the auto-retry branch; 0.3 s inter-step delay vs. 1.5 s in-place; single-shot per step; `emptyResult` never falls through; re-prime the full packet buffer on every step ≥1 and every retry; watchdog `min(300, 45 + secs*0.4)` and processing timeout `max(30, 30 + secs*0.5)`; `show_retrying(index+1, chain.len()+1)`; telemetry `fallback_hops`; manual retry resets to step 0 with a fresh retry budget; Provider settings UI with numbered rows, add/remove/reorder, promote/demote across the primary boundary, per-step OpenRouter model picker, and readiness badges.

### D. Secrets

Keep the keyring-first `CredentialStore` (it is better than the Swift file store for a signed shipping app), but add: accounts `openrouter-api-key` and `claude-code-token-mirror`; the three env-var overrides checked **before** the store; a value-free `has()` for the Saved badge; the read-only `Claude Code-credentials` reader with millisecond `expiresAt` and no skew margin; `clear_all_caches()`; delete-old-only-after-new-write-succeeds migration. Port the four-card Accounts UI with the exact test-connection endpoints, empty-means-keep semantics, and the explicit Check button for Claude Voice (never probe on mount).

### E. Polish gating

`Session::can_use_polish(primary_vendor)` = `primary_vendor == WisprFlow && session.is_valid()` (60 s expiry margin). Remove the Polish route from the settings nav entirely when false; skip polish-hotkey registration; gate the auto-polish branch and the manual handler; recompute on both settings-changed and session-changed; always pass the **primary** vendor, never the current chain step; never clear the stored polish settings.

### F. Keyterms

Port `ClaudeVoiceKeyTerms` with `NaturalLanguage.framework` FFI on macOS; the 65-word stopword set, `>= 4` chars, the numeric test, count-desc/key-asc ranking, limit 20, OCR-first-then-dictionary merge, exact-string dedup, and the one-recording-behind OCR hint delivered before `start()`. Extend `VocabSupport` with a prompt-injected variant for OpenRouter and a phrase-count budget alongside the token budget.
