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
- **Status:** done (commit 71e32d4) — ClaudeVoiceProvider with live PCM streaming, VoiceStream WS client (8s keepalive, anthropic-client-platform header), ClaudeCodeKeychain reader, ClaudeVoiceKeyTerms vocabulary extractor. Settings/status-bar already vendor-aware from B-008. OCR-keyterms wiring added in follow-up (commit f272742). Sibling repos and installed bundles fully retired on 2026-06-09 at user request: `claudia/` source (GitHub remote at cefege/claude-voice, still recoverable) and `wispr-edge/` source deleted from disk; `/Applications/{Claude Voice,Wispr Edge}.app` removed; `~/Library/Application Support/{Claude Voice,WisprEdge,Wispr Flow}/` and their log files removed. Lightning is now the only dictation app on the machine.
- **Depends on:** B-008

## B-010 — Onboarding window for Mac permissions

- **Type:** feature
- **Value:** high (new users hit silent failures otherwise)
- **Evidence:** Today `AppDelegate.applicationDidFinishLaunching` calls `AXIsProcessTrustedWithOptions(prompt: true)` and that's it. Microphone is requested lazily by the audio engine, Input Monitoring is never explicitly requested (the NSEvent global monitor silently no-ops until granted), Screen Recording only matters when `useScreenContext` is on. First-launch users see a hotkey that doesn't fire and have no idea why. Claudia (archived) shipped a clean `PermissionsManager` + `OnboardingWindow` pattern that this can be ported from.
- **Scope:** New `Services/PermissionsManager.swift` (Microphone / Input Monitoring / Accessibility / Screen Recording statuses + request actions + a `PermissionStatusPoller` that re-reads every 1s). New `UI/OnboardingWindow.swift` (480×600 SwiftUI sheet — bolt icon, one row per permission with status + Grant button). AppDelegate auto-shows on launch when any required permission is missing or `didCompleteOnboarding == false`. StatusBarController gets a "Setup & Permissions…" menu item to re-open. ~450 LOC new.
- **Status:** done (commit 776bf15) — all four permissions covered, auto-shows when missing, dismissible, status-bar re-entry. swift build green.

## B-011 — Tap-to-toggle hotkey mode

- **Type:** feature
- **Value:** medium
- **Evidence:** Today the only way to enter hands-free locked recording is the quick double-tap path in `AppDelegate.onHotkeyPress` (first press = listening, second press within 0.5s = lock, second press > 0.5s after = stop). Users who want a Wispr-Flow-like "tap once to start, tap again to stop" workflow have to learn the double-tap muscle memory or just hold the key. User asked for a setting to make a single short press behave like "click to start, click to stop".
- **Scope:** Add `hotkeyTapToToggle: Bool` to AppSettings (default false). When true: a quick press from idle enters `.recording` directly (skip `.listening` debounce). Subsequent press stops. Held keys still behave as PTT (release → stop) so existing users aren't disrupted. Surfaces as a row in Settings → General (under Shortcuts). ~30 LOC.
- **Status:** done (commit 5ce7784) — `hotkeyTapToToggle` setting, lock-on-quick-release path in `onHotkeyRelease`, Shortcuts panel toggle row. PTT (hold) behavior unchanged when the setting is off. Superseded by B-015's 3-mode picker on 2026-06-09.

## B-013 — Empty-state warnings in Provider chain

- **Type:** UX
- **Value:** medium
- **Evidence:** Primary + chain rows in Settings → Provider always show the picker cheerfully, even when the chosen vendor has no auth credentials. Result: first dictation fails with a confusing error.
- **Scope:** Add `DictationVendor.isReady(session:)` (Flow: session.isValid; OpenRouter: SecretsStore.has / Keychain hint; Claude Voice: best-effort file probe). Render a "Not signed in" badge next to misconfigured vendor pickers via a new `VendorReadinessBadge` view. Prompt-free check via `kSecReturnData: false`.
- **Status:** done (commit pending)

## B-014 — Hotkey conflict test field

- **Type:** UX
- **Value:** medium
- **Evidence:** If a user binds Lightning to a hotkey that's claimed by macOS or another app (Fn opens dictation, ⌥-space is Spotlight, etc.), Lightning silently doesn't fire. New users can't tell whether their key is bound or hijacked.
- **Scope:** `HotkeyConflictTester` SwiftUI view inside Shortcuts panel — NSEvent local monitor watches flagsChanged + keyDown, flashes "Detected: X" with a green check when the configured key arrives. Caveat text under the test field naming common conflicts.
- **Status:** done (commit pending)

## B-015 — Unified press-behavior picker

- **Type:** UX
- **Value:** medium
- **Evidence:** B-011 added a single tap-to-toggle bool but the three press modes (hold-only PTT, tap-to-toggle, legacy hold-or-double-tap) are all useful and aren't discoverable from a single checkbox.
- **Scope:** Replace `hotkeyTapToToggle` with `hotkeyPressBehavior: String` (default "legacy"), with values "hold" / "toggle" / "legacy". Migrate the bool on load. AppDelegate.onHotkeyRelease branches on the new field. Settings shows a radio-style picker with per-option hint text.
- **Status:** done (commit pending)

## B-016 — Onboarding ends with vendor pick

- **Type:** UX
- **Value:** high
- **Evidence:** After Get Started in the onboarding wizard, the user lands on the menu bar icon with no idea how to actually use the app. They have to discover Settings → Accounts on their own.
- **Scope:** Convert OnboardingView into a 3-step paged flow (permissions → mic test → vendor pick). Vendor pick step lets the user choose Wispr Flow / OpenRouter / Claude Voice as primary; selection writes to settings.activeVendor immediately. Sign-in still happens in Settings → Accounts (linked from the wizard finish CTA).
- **Status:** done (commit pending)

## B-017 — Mic test step in onboarding

- **Type:** UX
- **Value:** medium
- **Evidence:** Microphone permission is granted in step 1 but the user doesn't know if the selected input device actually works until they try a real dictation and get "No signal" / empty transcript.
- **Scope:** New `MicTestView` between permissions and vendor steps in the onboarding flow. Starts a temporary AudioRecorder, displays the live RMS level via `onLevelUpdate`, shows "Looks good" once level > threshold. Stops cleanly on disappear.
- **Status:** done (commit pending)
