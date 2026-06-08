# Wispr Lightning — Self-improvement backlog

Items proposed by `/propose`, picked off by `/improve <id>`. See `CLAUDE.md` for the loop overview.

---

## B-001 — Paste verification fails on every dictation (false negative)

- **Type:** bug
- **Severity:** high
- **Evidence:** `WisprLightning.log` shows `Paste verification failed — clipboard still restored` on **every** inject across the last several days, including dictations the user clearly accepted (kept dictating after). `verifyPaste` at `Sources/WisprLightning/Services/TextInjector.swift:345-362` reads the focused element's `kAXValueAttribute` and looks for `expected.prefix(20)`. Many targets (chat composers, contenteditable web fields, terminals, code editors) don't expose `AXValue` as a single readable string, so the check always returns `false` even when the paste landed.
- **Scope:** `Services/TextInjector.swift` ~30 LOC. Either (a) make verify treat "couldn't read value" as success (it already does for empty focus) and tighten the *positive* check, or (b) drop the verification entirely if the false-positive rate of "reverted clipboard breaks user's workflow" is low.
- **Status:** done (commit 518814e) — chose (b), dropped verification entirely.

## B-002 — Accessibility context never populates ("AX context: none" every time)

- **Type:** bug
- **Severity:** medium
- **Evidence:** `useAccessibilityContext: Bool = true` is the default in `Sources/WisprLightning/Models/Settings.swift:20`, and the README advertises context-aware formatting. But `WisprLightning.log` shows `AX context: none` on every single recording in the captured window. The log line is at `App/AppDelegate.swift:368`. The intended feature ("dictated text matches the style of what you're writing in") is silently disabled in practice.
- **Scope:** `App/AppDelegate.swift` (the call site) and `Services/AppInfoDetector.swift` (likely where AX text is supposed to be read). 1–2 files, ~50 LOC to investigate and fix the AX query.
- **Status:** wontfix — same AX limitation as B-001's verifyPaste. `kAXValueAttribute` is unset/non-string in most modern apps, so the query is effectively dead. A real fix needs either an AX hierarchy walk with per-bundle-ID heuristics or a pivot to OCR-only context. Documented in CLAUDE.md so the loop doesn't re-propose it.

## B-003 — WebSocket drops on recordings longer than ~90 seconds

- **Type:** bug
- **Severity:** low
- **Evidence:** `WisprLightning.log` 2026-05-07T14:27:16Z — a 90.5s recording (2262 packets) hit `WS receive failed: The operation couldn't be completed. Socket is not connected` on first send, then succeeded after auto-retry 1/2. User-visible effect: a few-second stall on long dictations. Root cause is almost certainly missing WebSocket keepalive / ping during recording.
- **Scope:** `Services/TranscriptionClient.swift` — add a ping timer on the open socket, ~15 LOC.
- **Status:** done (commit 61e9cd6) — 20s self-rearming DispatchWorkItem ping, +60 LOC, wired into all cancel sites.

## B-004 — Legacy single-key hotkey fields duplicate the array fields

- **Type:** refactor
- **Severity:** low
- **Evidence:** `Sources/WisprLightning/Models/Settings.swift:4-7` keeps `hotkeyKeyCode` / `hotkeyLabel` (commented "legacy single-key") alongside `hotkeyKeyCodes` / `hotkeyLabels`. Two sources of truth that can drift. If a future change updates one path but not the other, the hotkey UI and the listener could disagree.
- **Scope:** `Models/Settings.swift` + grep for legacy field readers and migrate them to the array form. Probably ~3 files, with a one-time migration in `load()`.
- **Status:** done (commit 4ab1707) — legacy fields kept for Codable compat, all live readers route to array form, one-time migration in `load()`.

## B-005 — Audio level meter in the recording pill

- **Type:** feature
- **Value:** medium
- **Evidence:** `UI/RecordingOverlay.swift` shows a static red dot pulsing at fixed opacity (0.6s in/out) while recording — no audio reactivity. Wispr Flow shows real-time mic input level. A daily user can't tell from the pill whether the mic is actually picking up their voice (e.g. on a meeting headset that dropped to default).
- **Scope:** `UI/RecordingOverlay.swift` + a tap into `Services/AudioRecorder.swift` to expose RMS level. ~40 LOC.
- **Status:** done (commit 56a1bf8) — red ring CALayer behind the dot, scales 1.0×–1.6× and fades 0–0.7 with smoothed RMS level.

## B-006 — Undo last dictation (revert + clipboard restore)

- **Type:** feature
- **Value:** medium
- **Evidence:** `App/StatusBarController.swift:90-107` already tracks `lastTranscription` and exposes it as a copy-to-clipboard menu item. There's no shortcut/menu item to undo the last inject — useful when AI formatting butchered a sentence or it landed in the wrong window. Wispr Flow has this.
- **Scope:** `App/StatusBarController.swift` + a small undo helper in `Services/TextInjector.swift` (post Cmd+Z to the previously focused app). ~30 LOC.
- **Status:** done (commit 98a1bc7) — "Undo last dictation" menu item posts Cmd+Z; clears `lastTranscription` so it disables itself after one use.

## B-007 — Lift `DictationProvider` abstraction into Lightning; gate polish behind Flow account

- **Type:** refactor
- **Value:** high (foundation for multi-vendor app)
- **Evidence:** `Sources/WisprLightning/Services/TranscriptionClient.swift` (546 LOC) is hardcoded against Wispr Flow's WebSocket and is called directly from `App/AppDelegate.swift` at the inject sites. `wispr-edge/Sources/WisprEdge/Services/DictationProvider.swift` already defines the protocol shape we want, but uses a batch (`packets: [Data]`) signature that doesn't fit Claude Voice's live-streaming model. Polish today is a first-class app concept (`Models/Settings.swift:28-44`, dedicated hotkey, separate Settings panel) even though Wispr Flow is the only backend that can do it. Continuing to surface polish UI for OpenRouter / Claude Voice users will be misleading.
- **Scope:**
  - New `Services/DictationProvider.swift` with **streaming** API: `start(context:)`, `feed(packet:)`, `stop(completion:)`, `cancel()`. `Context` carries appInfo + ocrContext + axContext + dictionaryStore.
  - New `Services/Providers/WisprFlowProvider.swift` — current `TranscriptionClient` becomes one implementation. Internally still buffers packets and uploads on `stop()` (no behavior change for Flow users).
  - `Services/AudioRecorder.swift` — feed PCM to the active provider on each chunk callback instead of accumulating to a flat array that gets handed to `TranscriptionClient` at the end.
  - `App/AppDelegate.swift` — talk to a `DictationProvider` instead of `TranscriptionClient` directly. Four call sites.
  - Polish gating: keep `PolishService` / `PolishStore` / polish hotkey code intact, but hide all polish UI in `UI/SettingsWindow.swift` and skip polish hotkey registration in `Services/HotkeyListener.swift` unless `Session` is authenticated as a Wispr Flow account. Settings model: leave fields, hide UI.
  - No vendor switching UX yet — Flow stays the only provider in B-007. B-008 / B-009 add the others.
  - ~250 LOC changed, ~150 LOC new, zero LOC deleted (polish kept, just gated).
- **Status:** done (commit 0808f00) — protocol + WisprFlowProvider + AudioRecorder onPacket + polish gating in place; swift build green.

## B-008 — Add OpenRouter (Gemini multimodal) as a second provider

- **Type:** feature
- **Value:** high
- **Evidence:** `wispr-edge/Sources/WisprEdge/Services/OpenRouterGeminiProvider.swift` (141 LOC) already implements the OpenRouter Gemini flow. Edge's `UI/SettingsWindow.swift:638-723` has the BYO-key UI (key field, test-connection button, model picker) we want. Lightning has no OpenRouter integration today.
- **Scope:**
  - Port `OpenRouterGeminiProvider` from Edge, adapt to the streaming protocol defined in B-007 (buffer internally, upload on `stop()`).
  - Port `Services/KeychainStore.swift` from Edge for storing the OpenRouter API key.
  - Add OpenRouter sub-panel in `UI/SettingsWindow.swift` (key field + secure entry + test + model picker), behind the vendor dropdown.
  - Add "Transcription provider" dropdown in Settings → General with options: Wispr Flow / OpenRouter (Gemini). Default to whatever's logged in; if both, last-used.
  - Add status bar "Provider →" submenu in `App/StatusBarController.swift` for quick switching.
  - Hide polish UI/hotkey when active provider is OpenRouter (extends B-007's gating logic).
  - Migrate Edge's pricing-disclaimer copy (`SettingsWindow.swift:655`).
  - ~400 LOC new (provider + UI + keychain).
- **Status:** done (commit 2b87b70) — DictationVendor enum, OpenRouterProvider, KeychainStore, AudioEncoding, Provider settings panel, status-bar Provider submenu. Polish gate is now `activeVendor == .wisprFlow && session.isWisprFlowAccount`.
- **Depends on:** B-007

## B-009 — Add Claude Voice as third provider (streaming); retire `claudia/` and `wispr-edge/`

- **Type:** feature
- **Value:** high
- **Evidence:** `/Users/mike/Documents/Code/claudia` has a working Claude Code STT client streaming PCM live to `wss://api.anthropic.com/api/ws/speech_to_text/voice_stream`, auth via `Claude Code-credentials` Keychain entry. Files to port: `Sources/TranscribeCore/VoiceStream.swift` (358 LOC, the WS client), `Sources/TranscribeCore/KeychainAuth.swift` (117 LOC), `Sources/ClaudiaApp/KeyTerms.swift` (NLTagger-based vocabulary boost, since Claude Code's API takes keyterms not full context). `claudia/CLAUDE.md` documents the load-bearing decisions (endpoint, streaming-not-batch, 8s keepalive, lazy Keychain read).
- **Scope:**
  - Port `VoiceStream`, `KeychainAuth`, `KeyTerms` into `Sources/WisprLightning/Services/Providers/ClaudeVoice/`.
  - New `ClaudeVoiceProvider: DictationProvider` — this is the one that uses the streaming protocol for real: opens WS on `start()`, sends PCM on every `feed()`, sends `CloseStream` on `stop()`.
  - Wire `KeyTerms` into the OCR/AX context path so Claude Voice gets vocabulary boost where Flow gets a context blob.
  - Extend vendor dropdown + status bar submenu with "Claude Voice (via Claude Code)" option. Login flow: "Run `claude /login`" hint, lazy Keychain read on first use (see claudia/CLAUDE.md #6).
  - Polish stays hidden for Claude Voice users (extends B-007/B-008 gating).
  - Update `CLAUDE.md` (Lightning's) to document the three-vendor architecture, lift over claudia's load-bearing decisions (endpoint, keepalive, no crash recovery, Keychain ownership).
  - After parity is confirmed via `/smoke`: delete `/Users/mike/Documents/Code/claudia/Claude Voice.app` from `/Applications`, archive the `claudia/` repo (or delete after user confirms), same for `wispr-edge/`.
  - ~700 LOC new (3 ported files + provider + KeyTerms wiring + settings UI + status bar).
- **Status:** done (commit 71e32d4) — ClaudeVoiceProvider with live PCM streaming, VoiceStream WS client (8s keepalive, anthropic-client-platform header), ClaudeCodeKeychain reader, ClaudeVoiceKeyTerms vocabulary extractor. Settings/status-bar already vendor-aware from B-008. OCR-keyterms wiring added in follow-up (commit f272742). Sibling repos moved to `_archived/` 2026-06-08: `claudia/` (GitHub remote at cefege/claude-voice, recoverable) and `wispr-edge/` (no remote, had uncommitted work, preserved in archive). `/Applications/Claude Voice.app` and `/Applications/Wispr Edge.app` left in place for the user to remove via Finder.
- **Depends on:** B-008
