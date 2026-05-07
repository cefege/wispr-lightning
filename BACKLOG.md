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
