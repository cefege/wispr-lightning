# Wispr Lightning — v2 Services Delta Spec (non-provider)

**Baseline the Rust port was built from:** `40532bf` (merge-base, branch `feature/natural-mode`).
**Current app:** `origin/feature/backlog-sweep` @ `8a81d74` — 40 commits, +7 136 / −415 lines ahead.

Source of every claim below is `git show origin/feature/backlog-sweep:<path>` or
`git diff 40532bf origin/feature/backlog-sweep -- <path>`. Provider internals
(`Services/DictationProvider.swift`, `Services/Providers/**`, `SecretsStore`,
`KeychainStore`, `OpenRouterModels`) are out of scope here — see the providers spec.
Pill/overlay/settings-window visuals are out of scope — see the UI v2 spec.

**Files that did NOT change between the two commits** (do not go looking for a delta):
`Services/AppInfoDetector.swift`, `Services/ScreenCaptureContext.swift`,
`Services/SoundManager.swift`, `Services/AuthService.swift`, `Services/MusicController.swift`,
`Services/NotesStore.swift`, `Services/PolishStore.swift`, `Constants.swift`.
`Services/PolishService.swift` changed by exactly one line (enum payload, §1.9).

---

## 0. Commit → change index

Every correction in this document is traceable to one of these:

| Commit | Subject | Sections |
|---|---|---|
| `75000be` | Send newlines as Shift+Return in Natural Mode | §1.3 |
| `d7cbe94` | Show 'Inserting…' on the pill while text is being typed | §1.4 |
| `3ee01bb` | Cancel Natural Mode typing on Esc | §1.5 |
| `6c243c4` | Tighten Natural Mode plumbing: non-optional settings, kVK_*, single main hop | §1.5, §1.10 |
| `518814e` | B-001: Drop paste verification | §1.1 |
| `181434a` | B-002: Close as wontfix; document AX limitation | §1.2 |
| `4ab1707` | B-004: route hotkey reads/writes through array fields only | §1.8, §7 |
| `56a1bf8` | B-005: Audio level meter in the recording pill | §1.7 |
| `98a1bc7` | B-006: Undo last dictation menu item | §1.6 |
| `5ce7784` | B-011: Tap-to-toggle hotkey mode | §1.11, §7 |
| `776bf15` | B-010: Onboarding wizard for Mac permissions | §3, §7 |
| `d0370a5` | B-013…B-017 (incl. B-015 press-behavior picker) | §1.11, §7 |
| `0eb2570` | /simplify — chain re-prime, audio callback race, leaks, dup | §8.5, §8.6 |
| `246ca4b` | /simplify — CV auth misroute, Session race, status-bar gap | §8.4 |
| `97bc7e7` | B-024…B-033: ten codebase hardening items | §8.1, §8.4, §6 |
| `299f8b2` | B-034..B-048: 15 production-hardening items | §2, §6, §8.9 |
| `fdec4d7` | /simplify — import data loss, audio engine race, sweep race | §8.2, §8.3, §8.7 |
| `b0b3899` | Three deferred /simplify follow-ups | §7 |
| `01a1f4c` | Items 1–7 from the standing audit | §1.2, §8.1, §8.8 |
| `fb41b57` | Three more polish items | §8.9 |
| `8a345a7` | /simplify — DB queue race, lockFocus thread guard | §8.1 |
| `2a88f2f` | Bound AX query latency + clean up migrate iteration | §1.2 |
| `8a81d74` | Deepgram + VU pill + audio-never-lost + lifecycle refactor (HEAD) | §2, §4, §5, §6 |

---

## 1. Behavioural corrections — things the Rust port implemented from the stale baseline

Each item: **port does now** → **current app does** → **who changed it**.

### 1.1 Paste verification is GONE (B-001, commit `518814e`)

**Port does now.** `crates/wl-platform/src/macos/injector.rs:513-542` pastes, sleeps
`PASTE_SETTLE`, then calls `verify_paste(text)` (`injector.rs:769-778`) which reads
`kAXValueAttribute` on the focused element and tests
`value.contains(expected.chars().take(VERIFY_PREFIX_CHARS))`. The result is returned
as the `bool` of `inject()`. `crates/wl-platform/src/windows/injector.rs:142-171` does the
same via UIA. `src-tauri/src/pipeline/mod.rs:429` logs
`"injection could not be verified"` on `Ok(Ok(false))`.

**Current app does.** `TextInjector.pasteViaClipboard` no longer verifies at all.
`verifyPaste(expected:)` was deleted outright, along with the
`Thread.sleep(forTimeInterval: 0.05)` that preceded it and the
`"Paste verification failed — clipboard still restored"` log line. The tail of the method
is now, verbatim:

```swift
// CGEvent.post can't tell us whether the focused app consumed Cmd+V.
// The previous AX-based verification produced ~100% false negatives in
// chat composers, contenteditable web fields, terminals, and code
// editors that don't expose AXValue, so it was driving spurious retry
// and error UI. Trust the post.
completion(true)
```

The `pasteSucceeded` flag is therefore **always `true` for the clipboard path**, and
`typeAsKeystrokes` also ends with `completion(true)`. The flag has exactly one live
consumer left — the audio-retirement branch in §2.4 — and under the current
implementation the `else` (keep-the-audio) arm is unreachable from the paste path.

**Consequence the port must not lose:** verification is not merely "relaxed", it is
*deleted*. Any Rust code that reasons about a `false` return from `inject()` is
reasoning about a state the Swift app can no longer produce.

### 1.2 Accessibility context: B-002 is `wontfix`, but the query was widened (`181434a`, then `01a1f4c` + `2a88f2f`)

**Port does now.** `MacInjector::read_focused_text` (`injector.rs:641-647`) reads exactly
one attribute — `ax::VALUE` — on the focused element and returns `vec![s]` if non-empty.
`Settings::use_accessibility_context` defaults to `true`
(`crates/wl-core/src/settings.rs:221`). The port treats AX context as a working feature.

**Current app does.** B-002 is closed **wontfix**. `CLAUDE.md` states as a load-bearing
convention:

> **AX context is reliably empty.** `TextInjector.readFocusedElementText()` reads
> `kAXValueAttribute` on the focused element. That attribute is unset or non-string in
> most modern apps (Slack, Cursor, Claude Code, terminals, web chat composers, document
> editors), so the runtime log shows `AX context: none` essentially always. […]
> `useAccessibilityContext` defaulting to `true` is currently aspirational.

`01a1f4c` item 6 then widened the query without changing the verdict. Exact current
implementation of `TextInjector.readFocusedElementText() -> [String]`:

1. `AXUIElementCreateSystemWide()`, then **`AXUIElementSetMessagingTimeout(systemWide, 0.05)`**.
2. `kAXFocusedUIElementAttribute` → return `[]` on failure.
3. `AXUIElementSetMessagingTimeout(element, 0.05)`.
4. Try, **in this order**, returning the first non-empty result as a single-element array:
   `kAXValueAttribute`, `kAXSelectedTextAttribute`, `kAXPlaceholderValueAttribute`,
   `"AXAttributedDescription"` (a raw `CFString`, no Carbon constant exists).
5. If all four miss: `kAXParentAttribute`, `AXUIElementSetMessagingTimeout(parentEl, 0.05)`,
   and re-try the **same four attributes in the same order** on the parent.
6. Otherwise `[]`.

`stringValue(_:attribute:)` accepts either `String` or `NSAttributedString` (using
`attr.string`), and rejects empty strings.

Latency budget is explicit in the source: 4 attributes × 2 levels = **8 worst-case IPCs
× 50 ms = 400 ms**, replacing an unbounded wait on a wedged target process (`2a88f2f`).

The call site is unchanged: `AppDelegate.startRecordingSession` dispatches onto
`axQueue` (`com.wisprlightning.ax`, `.userInitiated`) when `settings.useAccessibilityContext`
is true, and logs `AX context: none` or `AX context: <first 80 chars>...`.

### 1.3 Natural Mode newlines are **Shift+Return** (commit `75000be`)

**Port does now.** `injector.rs:611` — `'\n' => post_key(&source, VK_RETURN, CGEventFlags::empty(), hold)?`.
Windows `injector.rs:197-199` posts a bare `VK_RETURN` down/up pair. Both send an
unmodified Return.

**Current app does.** `TextInjector.postCharacter`, verbatim:

```swift
// Bare Return submits in most chat apps (Slack, Discord, ChatGPT,
// Claude Code's prompt) and executes in shells, so dictating a
// newline would send the message early. Shift+Return is the
// "newline without submit" convention. Raw shells still submit on
// either form — that's a known limitation.
if ch == "\n" {
    postKey(virtualKey: UInt16(kVK_Return), flags: [.maskShift], source: source)
    return
}
if ch == "\t" {
    postKey(virtualKey: UInt16(kVK_Tab), flags: [], source: source)
    return
}
```

`kVK_Return` = 36, `kVK_Tab` = 48. Tab stays unmodified. Flags are pinned
unconditionally on both the down and up event (`down.flags = flags; up.flags = flags`)
— this is the same load-bearing pinning `CLAUDE.md` warns against undoing.

### 1.4 An `Inserting…` pill state exists (commit `d7cbe94`)

**Port does now.** `OverlayState` (`src-tauri/src/ui.rs:14-34`) has
`Hidden | Recording | Locked | Processing | Retrying{attempt,of} | Error{message} | Recoverable{message}`.
There is no inserting state. `on_transcript` (`pipeline/transcribe.rs:196-199`) goes
straight from `Processing` to `inject().await` to `OverlayState::Hidden`.

**Current app does.** `RecordingOverlay.showInserting()` clears prior state (yellow
Retrying background, retry/save/dismiss buttons, time label) and shows a spinner
labelled **`"Inserting…"`**. It is built on a shared
`RecordingOverlay.showSpinner(label:width:)` extracted in `6c243c4`, which
`showProcessing()` also uses.

`CLAUDE.md` records this as load-bearing:

> **Pill state must be reset before each `TextInjector.inject` call.** Call
> `recordingOverlay.showInserting()` first; otherwise prior states (Retrying yellow,
> error buttons) bleed through. There are four inject call sites in `AppDelegate.swift`.

`CLAUDE.md` was written at `d7cbe94` time, when there were four. The lifecycle refactor
in `8a81d74` collapsed the two auto-polish exits into one `SafeCompletion` gate body, so
**at HEAD there are exactly three `textInjector.inject` call sites**, each immediately
preceded by `showInserting()`:

| Site | `AppDelegate.swift` | `showInserting()` | `inject` |
|---|---|---|---|
| Normal transcript | `handleTranscriptionResult` success, non-auto-polish branch | 829 | 830 |
| Polish hotkey | `onPolishHotkeyPress` success | 1292 | 1293 |
| Auto-polish | `autoPolishText`'s `SafeCompletion` gate body — reached by **either** the polish response **or** the 30 s watchdog, but only once (§4) | 1356 | 1357 |

Every one is followed by `DispatchQueue.main.async { self.recordingOverlay.hide() }`
inside the inject completion.

### 1.5 Esc cancels Natural Mode typing mid-stream (commits `3ee01bb`, `6c243c4`)

**Port does now.** `MacInjector::type_naturally` (`injector.rs:584-625`) is a plain
`while let Some(ch) = chars.next()` loop with no cancellation input and no focus check.
Nothing in `wl-platform` watches for Escape.

**Current app does.** `TextInjector` gained thread-safe cancellation state:

```swift
/// Flipped to `true` from the main-thread Esc monitor; read between
/// characters by the typing loop on `injectionQueue`.
private let cancelLock = NSLock()
private var _cancelTyping = false
```

with `setCancelTyping(_:)` / `isCancelTyping()` both taking `cancelLock`.

`typeAsKeystrokes(text:completion:)` now, in order:

1. `guard let source = CGEventSource(stateID: .privateState)` — on nil, log
   `"Natural Mode: failed to create CGEventSource — falling back to paste"` and
   `pasteViaClipboard`.
2. `setCancelTyping(false)`.
3. **One** `DispatchQueue.main.sync` block (folded from two in `6c243c4`) that installs
   both monitors and rebuilds the layout map:
   - `NSEvent.addGlobalMonitorForEvents(matching: .keyDown)` → if
     `Int(event.keyCode) == kVK_Escape` (53), `setCancelTyping(true)`. Observation only;
     Esc still reaches the focused app.
   - `NSEvent.addLocalMonitorForEvents(matching: .keyDown)` → same check, then
     `return nil` — **swallows** the keystroke inside Wispr Lightning's own windows.
   - `self.ensureLayoutMap()` (TIS asserts main thread).
4. `let initialFrontPID = NSWorkspace.shared.frontmostApplication?.processIdentifier`.
5. Log `"Natural Mode typing \(text.count) chars at \(cps) cps (layout map: \(layoutMap.count) entries)"`.
6. Per-character loop with `var typed = 0` and `let focusCheckInterval = 8`:
   - `if isCancelTyping()` → log
     `"Natural Mode: cancelled by Esc after \(typed)/\(text.count) chars"`, `break`.
   - `if typed > 0 && typed % 8 == 0` and the frontmost PID differs from
     `initialFrontPID` → log
     `"Natural Mode: focus changed mid-typing (pid \(old) → \(new)) — stopping after \(typed)/\(text.count) chars"`, `break`.
   - `postCharacter(ch, source:)`, `typed += 1`,
     `Thread.sleep(forTimeInterval: baseDelay * Double.random(in: 0.6...1.4))`.
7. `DispatchQueue.main.async { NSEvent.removeMonitor(...) }` for both monitors —
   the monitors exist only for the duration of one typing pass.
8. `completion(true)` — **cancellation still reports success**; partial output is not
   an error and the audio is retired normally.

The per-keystroke hold is unchanged: `Thread.sleep(forTimeInterval: Double.random(in: 0.030...0.080))`
between `down.post` and `up.post` in `postKey`.

The focus check in step 6 is item 6 of `299f8b2` (B-034..B-048), not part of `3ee01bb`.

### 1.6 "Undo last dictation" (B-006, commit `98a1bc7`)

**Port does now.** No undo anywhere. `src-tauri/src/tray.rs` menu labels are exactly:
`Input Device`, `System Default`, `Pause hotkey` / `Resume hotkey`, `Natural Mode`,
`Settings`, `Quit Wispr Lightning`, `No recent dictation`.

**Current app does.** `TextInjector.undoLastInjection()`:

```swift
func undoLastInjection() {
    let source = CGEventSource(stateID: .hidSystemState)
    guard let keyDown = CGEvent(keyboardEventSource: source, virtualKey: 6, keyDown: true),
          let keyUp = CGEvent(keyboardEventSource: source, virtualKey: 6, keyDown: false) else {
        wLog("Failed to create Cmd+Z CGEvent — check Accessibility permissions")
        return
    }
    keyDown.flags = .maskCommand
    keyUp.flags = .maskCommand
    keyDown.post(tap: .cghidEventTap)
    keyUp.post(tap: .cghidEventTap)
    wLog("Cmd+Z posted (undo last dictation)")
}
```

Virtual key **6** is `Z`. Source state is **`.hidSystemState`**, deliberately *not*
`.privateState` — the doc comment says Cmd+Z is "a one-shot system shortcut, not a
Natural Mode character". No inter-key sleep.

Status-bar wiring (`StatusBarController`):

```swift
let undoItem = NSMenuItem(title: "Undo last dictation", action: #selector(undoLastDictation), keyEquivalent: "")
undoItem.target = self
undoItem.isEnabled = !(lastTranscription?.isEmpty ?? true)
menu.addItem(undoItem)
```

placed immediately after the last-transcription preview block and **before** the
`Recent dictations` submenu. Handler:

```swift
@objc private func undoLastDictation() {
    guard let text = lastTranscription, !text.isEmpty else { return }
    textInjector.undoLastInjection()
    lastTranscription = nil
    buildMenu()
    wLog("Undo last dictation — \(text.count) chars")
}
```

One-shot: `lastTranscription` is cleared so a second press cannot over-undo into the
user's prior text. `StatusBarController.init` gained a `textInjector: TextInjector`
parameter for this.

### 1.7 The overlay HAS a level meter, and `AudioRecorder` publishes RMS (B-005 `56a1bf8`, superseded by `8a81d74`)

The old `platform-spec.md` §1 states, under "Audio level / RMS — IMPORTANT CORRECTION",
that "`AudioRecorder` computes and publishes NO audio level at all". **That is now false.**

`AudioRecorder` gained:

```swift
/// Called from the audio capture thread (NOT main) once per buffer with a
/// 0.0–1.0 normalized RMS level. UI consumers must hop to the main queue.
/// Set to nil when not recording to avoid keeping references alive.
var onLevelUpdate: ((Float) -> Void)?

/// Called from the audio capture thread (NOT main) for every 40ms PCM
/// packet captured during recording.
var onPacket: ((Data) -> Void)?
```

`onLevelUpdate` fires **once per hardware tap buffer**, computed from the *raw hardware*
buffer (before conversion), inside the tap callback right after `processBuffer`:

```swift
private static func computeNormalizedLevel(from buffer: AVAudioPCMBuffer) -> Float {
    let frameLength = Int(buffer.frameLength)
    guard frameLength > 0 else { return 0 }
    var sumSquares: Float = 0
    if let floatPtr = buffer.floatChannelData?[0] {
        for i in 0..<frameLength { let s = floatPtr[i]; sumSquares += s * s }
    } else if let int16Ptr = buffer.int16ChannelData?[0] {
        let scale: Float = 1.0 / 32768.0
        for i in 0..<frameLength { let s = Float(int16Ptr[i]) * scale; sumSquares += s * s }
    } else { return 0 }
    let rms = sqrtf(sumSquares / Float(frameLength))
    guard rms > 0 else { return 0 }
    let db = 20.0 * log10f(rms)
    let clamped = max(-60.0, min(0.0, db))
    return (clamped + 60.0) / 60.0
}
```

Exactly: channel 0 only, Float32 preferred over Int16, unsupported formats → `0`,
**−60 dBFS → 0.0, 0 dBFS → 1.0**, linear in dB.

`onPacket` fires per converted 640-sample / 1280-byte packet, from inside the chunking
loop in `processBuffer`, **after** `packets.append(data)` and **outside** `packetsLock`.

B-005 (`56a1bf8`) rendered this as a red ring CALayer behind the dot (1.0×–1.6× scale,
0–0.7 opacity). The HEAD commit `8a81d74` **replaced that** with a 21-bar VU strip driven
by a rolling RMS buffer, bars being the sole Recording indicator (no dot, no "Listening"
text), green when locked. See the UI v2 spec for the rendering; the service contract is
what is specified here.

### 1.8 Legacy single-key hotkey fields are dead for readers (B-004, commit `4ab1707`)

**Port does now.** `Settings.legacy_hotkey_key_codes` is `#[serde(rename = "hotkeyKeyCodes")]`
and is migrated then **cleared** (`settings.rs:338-349`). There is no `hotkeyKeyCode` /
`hotkeyLabel` scalar at all.

**Current app does.** The scalars survive on `AppSettings` **only** for Codable
backward-compat and are annotated as such:

```swift
// Deprecated — kept for Codable backward-compat. All readers use the array form.
var hotkeyKeyCode: UInt16 = 59
var hotkeyLabel: String = "Left Control"
```

Every live reader now uses array-with-literal-fallback:

- `HotkeyListener.rebuildHotkeySet()`: `_hotkeySet = codes.isEmpty ? [59] : Set(codes)`
  (was `[settings.hotkeyKeyCode]`).
- `HotkeyListener.installMonitors()`: `let labels = settings.hotkeyLabels.isEmpty ? ["Left Control"] : settings.hotkeyLabels`
  (was `[settings.hotkeyLabel]`).
- `HotkeyListener.rebind(keyCode:)` no longer writes the scalars at all:
  ```swift
  let label = Self.keycodeLabels[keyCode] ?? "Key \(keyCode)"
  settings.hotkeyKeyCodes = [keyCode]
  settings.hotkeyLabels = [label]
  settings.save()
  ```
- `AppDelegate` launch line: `wLog("Ready — press \(settings.hotkeyLabels.first ?? "Left Control") to start dictating")`.

The one-time migration lives in `AppSettings.applyMigrations` — see §7.4 for why it is
effectively dead code.

### 1.9 `TranscriptionError.authFailed` now carries a payload

`enum TranscriptionError` changed from `case authFailed` to
**`case authFailed(String?)`**, plus a new computed property:

```swift
/// True when failure on this provider should automatically try the next
/// chain step (auth, network, server, timeout — anything except
/// `emptyResult`, which usually means the mic didn't catch speech and
/// a different model won't help).
var shouldFallback: Bool {
    switch self {
    case .authFailed, .connectionFailed, .timeout, .serverError: return true
    case .emptyResult: return false
    }
}
```

`userMessage` for `.authFailed(let detail)` is `detail ?? "Authentication failed — please sign in again"`.
The only non-provider caller updated was `PolishService.polish` →
`completion(.failure(.authFailed(nil)))`.

### 1.10 `TextInjector.init` takes a non-optional `AppSettings` (commit `6c243c4`)

`init(settings: AppSettings? = nil)` → `init(settings: AppSettings)`. Every
`settings?.x ?? default` read collapsed to a direct read
(`self.settings.naturalModeEnabled`, `charsPerSecond(for: settings.naturalModeSpeed)`).
Mechanical, but it removes the "no settings → paste mode" fallback the port may have
mirrored.

### 1.11 Tap-to-toggle, then the three-mode press-behavior picker (B-011 `5ce7784`, superseded by B-015 `d0370a5`)

**Port does now.** `crates/wl-core/src/fsm.rs` implements exactly one behaviour — the
legacy hold-or-double-tap model — and hard-codes it. The module doc comment enumerates
it as "the interaction model, which is subtle and must be preserved exactly". There is no
settings input to `Machine::handle`.

**Current app does.** `AppDelegate.onHotkeyRelease()` branches on
`settings.hotkeyPressBehavior`. Full current body, with the constants
`lockDebounceInterval = 0.5`, `trailingBufferInterval = 0.5`:

```
guard recordingState == .listening else { return }      // locked mode ignores release
heldDuration = now - lastPressTime  (default 1.0 if lastPressTime is nil)
behavior = settings.hotkeyPressBehavior

if heldDuration >= 0.5:
    // Long hold = PTT in ALL THREE modes. Identical to baseline.
    tapDelayTimer = Timer(0.5) { if state == .listening { stopRecordingSession() } }
    return

switch behavior:
  "hold":    tapDelayTimer.invalidate(); tapDelayTimer = nil; stopRecordingSession()
  "toggle":  tapDelayTimer.invalidate(); tapDelayTimer = nil
             recordingState = .recording
             lastPressTime = Date()
             wLog("Recording locked — tap-to-toggle mode")
             recordingOverlay.showLocked()
  default:   // "legacy" — baseline behaviour, unchanged
             remaining = 0.5 - heldDuration
             tapDelayTimer = Timer(remaining) { if state == .listening { stopRecordingSession() } }
```

Note the asymmetry the port must reproduce: **`"hold"` stops immediately on a quick tap
with no trailing buffer at all**, while a genuine hold gets the 0.5 s trailing buffer.

`onHotkeyPress()` is unchanged from baseline except that the two stop paths
(slow second press in `.listening`, and any press in `.recording`) now route through
`stopRecordingSessionWithTrailingBuffer()` — see §2.5.

The Settings UI (`SettingsWindow.swift:436-448`) renders a `.radioGroup` picker with
exactly these three rows and tags:

| Label | Tag | Hint text |
|---|---|---|
| `Hold to talk` | `"hold"` | `Recording lasts as long as the key is held. Releasing always ends it.` |
| `Tap to start, tap to stop` | `"toggle"` | `Press once to start, press again to stop. Holding still works as push-to-talk.` |
| `Hold or double-tap to lock (legacy)` | `"legacy"` | `Quick tap waits for a second tap to lock hands-free. Hold longer than ~0.5s for push-to-talk.` |

`SettingsViewModel.save…` keeps the deprecated bool mirrored:
`settings.hotkeyTapToToggle = (hotkeyPressBehavior == "toggle")` (`SettingsWindow.swift:2369`).
Its load-time read is `settings.hotkeyPressBehavior.isEmpty ? (settings.hotkeyTapToToggle ? "toggle" : "legacy") : settings.hotkeyPressBehavior`.

### What the Rust port must change (§1)

1. **Delete `verify_paste` from both platform injectors** and every constant it needs
   (`VERIFY_PREFIX_CHARS`, `PASTE_SETTLE` if unused elsewhere). `TextInjector::inject`
   for `InjectMode::Paste` must return `Ok(true)` unconditionally once the Cmd+V/Ctrl+V
   post succeeds. Remove the `Ok(Ok(false)) => "injection could not be verified"` arm in
   `pipeline/mod.rs:429` or reduce it to a Natural-Mode-only path.
2. **Widen `read_focused_text`** to the exact four-attribute × two-level ladder in §1.2,
   with `AXUIElementSetMessagingTimeout(_, 0.05)` on each element, and add the
   `NSAttributedString` acceptance. Then document in the Rust source that
   `use_accessibility_context: true` is aspirational — B-002 is wontfix, not open.
3. **`'\n'` must post Return with the Shift modifier** on macOS
   (`CGEventFlags::MaskShift`) and Windows (`VK_SHIFT` down / Return / `VK_SHIFT` up).
   Tab stays unmodified. Keep the modifier pinned on both down and up events.
4. **Add `OverlayState::Inserting`** and set it immediately before each of the three
   `inject()` call sites — the transcript path, the polish-hotkey path, and the
   auto-polish gate body — then `Hidden` in the completion. The state must clear Retrying
   styling and any retry/save/dismiss buttons.
5. **Add Escape cancellation and mid-typing focus tracking** to `type_naturally`: an
   `AtomicBool` checked between characters, a global Escape observer plus a local one
   that swallows the key, a frontmost-PID snapshot re-checked every 8 characters, and
   monitor teardown at loop exit. Cancellation must still return `Ok(true)`.
6. **Add `undo_last_injection()`** to the `TextInjector` trait (Cmd+Z on macOS via
   `hidSystemState` + virtual key 6, Ctrl+Z on Windows) and an **`Undo last dictation`**
   tray item, enabled only when a last transcription exists, that clears it after firing.
7. **`AudioRecorder` must publish a per-buffer normalized level** using the exact
   −60…0 dBFS mapping in §1.7, computed on the raw hardware buffer, and the overlay must
   render it. Delete the "no level meter" claim from `platform-spec.md` §1.
8. Keep the array-only hotkey read path (the port already does this), but see §7 for the
   *serialization* hazard it creates.
9. `ProviderError`'s auth variant needs an optional vendor-specific message, and a
   `should_fallback()` predicate distinct from `is_retryable()` —
   `EmptyResult` is retryable-adjacent but must **never** advance a fallback chain.
10. **Implement all three press behaviours** in `wl-core::fsm`. `Machine::handle` needs the
    behaviour as an input (constructor parameter or per-event argument, but it must be
    re-readable so a settings change mid-session takes effect). Preserve exactly: hold
    ≥ 0.5 s is PTT in all three modes; `"hold"` quick-tap stops with **zero** delay;
    `"toggle"` quick-tap locks; `"legacy"` schedules the stop at `0.5 − held`.

---

## 2. `RecordingArtifact` and the audio-never-lost hardening (`8a81d74`, plus items 3 & 5 of `299f8b2`)

### 2.1 The type

`Sources/WisprLightning/Services/RecordingArtifact.swift`, 93 lines, `final class`.
It replaces four scattered `AppDelegate` ivars: `activeRecordingFileHandle`,
`activeRecordingFileURL`, `pendingAudioFileURL`, and a free-floating `recordingIOQueue`.

```swift
final class RecordingArtifact {
    let url: URL
    private var liveHandle: FileHandle?
    private let ioQueue: DispatchQueue
```

**Two constructors, and only two:**

| Constructor | Semantics |
|---|---|
| `init?(creatingAt url: URL)` | Live-write mode. `FileManager.default.createFile(atPath:contents: nil)` **and** `FileHandle(forWritingAtPath:)` must both succeed; otherwise `try? FileManager.default.removeItem(at: url)` and **return nil**. Queue label `"com.wisprlightning.recording.io"`. |
| `init(capturedAt url: URL)` | Recovery mode. Non-failable. `liveHandle = nil` — `append()` is a permanent no-op. The queue is still created "just so `delete()` can use the same sync-drain pattern uniformly". |

**Four methods:**

- **`append(_ packet: Data)`** — `ioQueue.async { [weak self] in … }`. Guards on
  `self.liveHandle`. On a `write(contentsOf:)` throw it logs
  `"Wispr Lightning: incremental audio write failed: %@"` via `NSLog`, closes the handle
  and sets `liveHandle = nil` so subsequent writes silently no-op against a dead
  descriptor. The in-memory packets array is unaffected.
- **`finishWriting()`** — `ioQueue.sync { try? liveHandle?.close(); liveHandle = nil }`.
  **Idempotent.** The synchronous drain is the guarantee that every queued packet has
  landed before the file is handed onward. The file **stays on disk**.
- **`delete()`** — `finishWriting()` then `try? FileManager.default.removeItem(at: url)`
  then `wLog("Deleted saved audio: \(url.lastPathComponent)")`. **Idempotent.**
- **`deleteAfter(_ delay: TimeInterval)`** —
  `DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + delay) { self.delete() }`.
  The **strong `self` capture is deliberate and documented**: the caller's last reference
  (`artifactToRetire`, a stack local) usually dies immediately, and `pendingAudio` has
  already been nilled, so a `[weak self]` capture would let the artifact deallocate and
  silently skip the delete, leaving the file until the 24 h sweep.

### 2.2 On-disk location and format

- Directory: `AppDelegate.pendingAudioDir` =
  `FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first`
  (falling back to `~/Library/Application Support`) `+ "WisprLightning/PendingAudio"`,
  created with `withIntermediateDirectories: true`. **`.first` is now guarded** — the
  force-unwrap was one of the fixes in `8a81d74`.
- Filename: `"recording-\(logDateFormatter.string(from: Date())).pcm"` where
  `logDateFormatter` is a bare `ISO8601DateFormatter()` — i.e.
  `recording-2026-06-16T05:31:07Z.pcm`. **The filename contains colons.**
- Format: bare concatenation of 1280-byte packets (640 × Int16 LE, 16 kHz mono). No
  header. `loadAudioFromDisk` reconstructs by `data.subdata(in: offset..<offset+1280)`
  while `offset + 1280 <= data.count`; a trailing partial packet is discarded.

### 2.3 When the artifact is created and written

Creation happens at **recording start**, not stop — this is the core of the hardening.
In `startRecordingSession()`, before any callback is wired:

```swift
let filename = "recording-\(logDateFormatter.string(from: Date())).pcm"
let url = Self.pendingAudioDir.appendingPathComponent(filename)
pendingAudio = RecordingArtifact(creatingAt: url)
if pendingAudio == nil {
    wLog("Failed to create incremental audio file at \(url.lastPathComponent) — proceeding without disk snapshot")
}
```

Then, still before `audioRecorder.start()`:

```swift
audioRecorder.onPacket = { [weak self] packet in
    guard let self = self else { return }
    self.dictationProvider.feed(packet: packet)
    self.pendingAudio?.append(packet)
}
audioRecorder.onLevelUpdate = { [weak self] level in
    DispatchQueue.main.async { self?.recordingOverlay.updateAudioLevel(level) }
}
```

So **every 40 ms packet is appended to disk as it is captured**, on the artifact's own
serial I/O queue. A mid-record crash leaves a valid, complete-up-to-that-point `.pcm`.

If `audioRecorder.start()` returns `.failed`, the callbacks are cleared and the provider
cancelled before returning (§8.6) — but note the artifact is *not* deleted on that path;
it is a zero-byte file that the next `clearPendingTranscription` sweep or the 24 h sweep
will eventually collect.

### 2.4 When the artifact is retired — every call site

`clearPendingTranscription()` **no longer deletes the file**. The old
`clearPendingTranscription(deleteAudio:)` bool parameter was dropped entirely; the HEAD
commit calls it out as "removing a class of default-true footguns". Its doc comment:

> Reset transcription state. Does NOT delete the on-disk audio file — each caller decides
> explicitly by calling `pendingAudio?.delete()` / `.deleteAfter(_:)` (or capturing the
> artifact reference for deferred deletion).

Complete list of terminal decisions:

| Site | Decision | Rationale in source |
|---|---|---|
| `stopRecordingSession()`, `packets.count < 5` | `pendingAudio?.delete(); pendingAudio = nil` | "no value in keeping <200ms of audio" |
| `handleTranscriptionResult` success, non-empty text, **no auto-polish** | capture `let artifactToRetire = self.pendingAudio` **before** `clearPendingTranscription()`; then `artifactToRetire?.delete()` **inside the `inject` completion**, only when `pasteSucceeded == true` | audio survives an inject failure |
| …same, `pasteSucceeded == false` | **keep the file**, log `"Inject reported failure — keeping audio file for recovery: \(url.lastPathComponent)"` | focused field gone / accessibility revoked |
| `handleTranscriptionResult` success, **auto-polish** branch | `artifactToRetire?.deleteAfter(60)` | "polish runs async and might hang; we want a grace window" |
| `handleTranscriptionResult` success, **empty** transcript | `artifactToRetire?.delete()` immediately | "the same providers will return the same empty result … the 24h sweep would only delay the same outcome" |
| `dismissRetry()` | `pendingAudio?.delete()` then `clearPendingTranscription()` | user explicitly declined after seeing Save |
| `abortRecording(reason:)` (sleep, pill ✕) | `pendingAudio?.finishWriting()` — **kept** | "recovery on the next launch can offer the user to retry" |
| `applicationWillTerminate` | `pendingAudio?.finishWriting()` — **kept** | "recovery scans for exactly this case" |
| 24 h sweep | delete | see §2.6 |

The delete-only-after-inject ordering is the fix; the source states the prior behaviour
explicitly: "Previously we cleared (and deleted the file) before inject ran — if inject
failed, crashed, or returned thin results, the user lost both the transcript and the
source audio with no way to retry."

### 2.5 The other audio-never-lost changes in the same commit

**Trailing tail on tap-to-stop.** New constant
`toggleStopTrailingBuffer: TimeInterval = 0.25` and:

```swift
private func stopRecordingSessionWithTrailingBuffer() {
    tapDelayTimer?.invalidate()
    tapDelayTimer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: false) { [weak self] _ in
        guard let self = self else { return }
        guard self.recordingState == .recording || self.recordingState == .listening else { return }
        self.stopRecordingSession()
    }
}
```

Audio capture continues during the 0.25 s. Both `.listening`-slow-second-press and
`.recording`-press paths in `onHotkeyPress` now use it. The guard exists so a pill-✕
click inside the window (which already ran `abortRecording`) cannot re-enter.

**Per-provider watchdog.** In `attemptTranscription()`:

```swift
private static let perProviderWatchdogBase: TimeInterval = 45
private static let perProviderWatchdogPerSecond: TimeInterval = 0.4
private static let perProviderWatchdogCap: TimeInterval = 300

let recordingSeconds = Double(packets.count) * Double(Constants.chunkDurationMs) / 1000.0
let watchdogSeconds = min(300, 45 + recordingSeconds * 0.4)
let watchdog = DispatchWorkItem { [weak self] in
    wLog("Provider watchdog fired after \(Int(watchdogSeconds))s — forcing fallback")
    self?.attemptWatchdogFired = true
    self?.dictationProvider.cancel()
    gate.fire(.failure(.timeout))
}
DispatchQueue.global().asyncAfter(deadline: .now() + watchdogSeconds, execute: watchdog)

dictationProvider.stop(context: context) { result in
    watchdog.cancel()
    gate.fire(result)
}
```

This is **per provider attempt** and is distinct from the pre-existing whole-pipeline
`scheduleProcessingTimeout()`, which is unchanged: `max(30.0, 30.0 + recordingDuration * 0.5)`,
a main-thread `Timer`, showing `showRetryableError(message: "Timed out", …)`.

**Recovery auto-retry.** `recoverPendingAudio()` at launch:

- Scan `pendingAudioDir` for `*.pcm`, sort by `.creationDateKey` descending, take the newest.
- `fileAge > 86400` → delete **all** pcm files, return.
- `loadAudioFromDisk` failure → delete that file, return.
- Otherwise: `wLog("Recovered \(packets.count) packets from previous session: \(name)")`,
  set `pendingPackets`, `pendingAudio = RecordingArtifact(capturedAt: mostRecent)`,
  `pendingAppInfo = ["name": "Unknown", "bundle_id": "", "type": "other", "url": ""]`,
  `currentRetryAttempt = 0`, **`currentChainIndex = 0`** ("Recovery always starts from the
  primary vendor").
- **`if fileAge < 90`** → `wLog("Recovered file is \(Int(fileAge))s old — auto-retrying transcription silently")`
  and `DispatchQueue.main.async { self.retryTranscription() }`.
- Else → `recordingOverlay.showRetryableError(message: "Recovered unsent recording", onRetry:onSave:onDismiss:)`.
- Finally, delete every other `.pcm` in the directory.

**Mid-recording mic disconnect now stops the session.** The `.audioDevicesChanged`
observer previously only logged. Now:

```swift
wLog("Target mic '\(self.settings.micDeviceName ?? targetUID)' disconnected during recording — stopping session")
self.stopRecordingSession()
```

Rationale in source: "Previously we just logged and let the engine keep capturing against
whatever device CoreAudio fell back to — silently producing wrong-source audio."

**`saveAudioToDisk(_:)` is now dead code.** It survives at `AppDelegate.swift:1128-1143`
with no callers — the incremental path replaced it. `loadAudioFromDisk(_:)` is still live
(recovery only).

### 2.6 The 24 h sweep

```swift
private func sweepStalePendingAudio(activePath: String?) {
    guard let files = try? FileManager.default.contentsOfDirectory(
        at: Self.pendingAudioDir, includingPropertiesForKeys: [.creationDateKey]) else { return }
    let now = Date()
    for file in files where file.pathExtension == "pcm" {
        if let active = activePath, file.path == active { continue }
        guard let created = (try? file.resourceValues(forKeys: [.creationDateKey]).creationDate),
              now.timeIntervalSince(created) > 86400 else { continue }
        try? FileManager.default.removeItem(at: file)
    }
}
```

Kicked from the tail of `clearPendingTranscription()` on
`DispatchQueue.global(qos: .background)`. String comparison, not `URL` equality, is
deliberate (§8.3).

> **Latent bug, stated for accuracy, not to be reproduced:** in
> `clearPendingTranscription()` the line `pendingAudio = nil` (`AppDelegate.swift:1081`)
> runs *before* `let activePath = pendingAudio?.url.path` (`:1101`), so `activePath` is
> always `nil` at that call site. Harmless today because the sweep only touches files
> older than 24 h, and the just-cleared artifact is seconds old. The Rust port should
> snapshot the path *before* clearing.

### 2.7 Comparison with `src-tauri/src/spool.rs`

| Dimension | Swift `RecordingArtifact` + `AppDelegate` | Rust `Spool` (`src-tauri/src/spool.rs`) |
|---|---|---|
| **When audio hits disk** | Incrementally, per 40 ms packet, from `startRecordingSession()` onward | Once, at `stop_recording` via `spawn_blocking(spool.save(&packets))` (`pipeline/actor.rs:310`) — nothing on disk during recording |
| **Crash mid-recording** | Complete-up-to-crash `.pcm` recoverable | **Entire recording lost** |
| **Write mechanism** | `FileHandle.write(contentsOf:)` on a serial queue, handle held open | `fs::write` to `<name>.part` then `fs::rename` — atomic, but all-at-once |
| **Handle lifecycle** | `finishWriting()` sync-drains and closes; idempotent | No handle |
| **Filename** | `recording-<ISO8601 with colons>.pcm` | `recording-YYYYMMDD-HHMMSS-mmm.pcm` (`timestamp()` + `subsec_millis()`) |
| **Directory** | `~/Library/Application Support/WisprLightning/PendingAudio` | Same (`wl_core::paths::pending_audio_dir()`) — compatible |
| **Deletion trigger** | `artifactToRetire?.delete()` **inside the inject completion**, gated on success | `discard_pending(&deps)` at the **top** of `on_transcript` (`transcribe.rs:177`) — *before* injection, and unconditionally |
| **Inject-failure survival** | File kept, logged | **File already gone** |
| **Auto-polish grace window** | `deleteAfter(60)` | None — `discard_pending` already ran |
| **Empty-transcript handling** | Explicit immediate `delete()` | Falls out of the same unconditional `discard_pending` |
| **Recovery age window** | 24 h (`86400`), from `.creationDateKey` | 24 h (`MAX_AGE`), from `metadata().modified()` |
| **Recovery auto-retry** | Silent `retryTranscription()` when `fileAge < 90` s | None — always `pipeline.offer_recovery()` → user-visible pill |
| **Min-length gate on recovery** | None (any file that parses to ≥ 1 packet) | `packets.len() >= MIN_PACKETS`, else delete |
| **Other-file cleanup at recovery** | Deletes every non-newest `.pcm` | Same (`recover_latest` deletes all but newest) |
| **Opportunistic 24 h sweep between launches** | Yes, after every dictation | **None** — only the launch-time `recover_latest` prunes |
| **Truncated-file tolerance** | Trailing partial packet dropped in `loadAudioFromDisk` | `chunks_exact(CHUNK_BYTES)` — same |
| **Ownership** | One typed object owns url + handle + queue | `Spool` is a directory handle; the path lives in `Pending.spool_path: Option<PathBuf>` and is filled in **asynchronously** after `stop_recording` returns |
| **Race guard on the async save** | N/A (synchronous creation at start) | `Arc::ptr_eq(&pending.packets, &to_spool)` — if a second dictation replaced `pending`, the just-written file is deleted (`actor.rs:316-322`) |

The two designs disagree on the single most important point: **the Rust port deletes the
spool file before it knows the text was delivered, and never has a spool file at all while
the user is speaking.** Both are the behaviours `8a81d74` was written to eliminate.

### What the Rust port must change (§2)

1. **Introduce an owning artifact type** (`RecordingArtifact` equivalent) holding
   `PathBuf` + `Option<File>` + a serial writer, with the two construction modes
   (`create_at` → `Option<Self>`, `captured_at` → `Self`) and idempotent
   `finish_writing()` / `delete()` / `delete_after(Duration)`. Replace
   `Pending.spool_path: Option<PathBuf>` and the `Arc::ptr_eq` reconciliation with it.
2. **Open the file at recording start and append every packet as it arrives.** Wire the
   append into the same callback that feeds the provider, before the capture engine
   starts. On write error, drop the handle and continue — never fail the recording.
3. **Move deletion into the inject completion** and gate it on the injection result.
   `discard_pending` must no longer run at the top of `on_transcript`. Split it into
   "reset pipeline state" and "retire the artifact", with the artifact captured before
   the reset.
4. Add the **auto-polish 60 s grace window** and the **immediate delete on empty
   transcript**.
5. **Preserve the file on abort and on process exit** (`finish_writing`, never `delete`).
6. Add the **per-provider watchdog**: `min(300, 45 + duration_secs * 0.4)`, which cancels
   the provider and synthesizes a timeout, distinct from the existing processing deadline
   (`tokio::time::timeout` in `drive_transcription`). Record that it fired (§5).
7. Add the **`fileAge < 90 s` silent auto-retry** to the recovery path; keep the explicit
   pill for older files. Reset the chain index to 0 on recovery.
8. Add the **24 h opportunistic sweep** after each completed dictation, snapshotting the
   active path *before* clearing pending state.
9. Add the **0.25 s trailing tail on tap-to-stop**, distinct from the 0.5 s hold-PTT
   trailing buffer.
10. **Stop the session on mid-recording mic disconnect** rather than continuing against
    the fallback device.

---

## 3. `PermissionsManager` (B-010, commit `776bf15`; poller consumers in `97bc7e7`)

New file `Sources/WisprLightning/Services/PermissionsManager.swift`, 164 lines.
Imports `AppKit`, `AVFoundation`, `ApplicationServices`, `Combine`, `CoreGraphics`,
`IOKit.hid`.

### 3.1 The model

```swift
enum PermissionStatus: Equatable { case granted, notDetermined, denied }
enum Permission: CaseIterable { case microphone, inputMonitoring, accessibility, screenRecording }
```

`Permission.allCases` order is **microphone, inputMonitoring, accessibility,
screenRecording** — that is the order the onboarding rows render in.

| Permission | `title` | `rationale` | `isRequired` |
|---|---|---|---|
| `.microphone` | `Microphone` | `Record your voice for dictation.` | `true` |
| `.inputMonitoring` | `Input Monitoring` | `Listen for your global push-to-talk hotkey when other apps are focused.` | `true` |
| `.accessibility` | `Accessibility` | `Paste transcripts at the cursor and type characters in Natural Mode.` | `true` |
| `.screenRecording` | `Screen Recording` | `Optional — read on-screen text as transcription context. macOS will quit Wispr Lightning after you grant this; relaunch from /Applications.` | `false` |

`systemSettingsURL` (force-unwrapped `URL(string:)`, all four are valid):

| Permission | URL |
|---|---|
| `.microphone` | `x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone` |
| `.inputMonitoring` | `x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent` |
| `.accessibility` | `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility` |
| `.screenRecording` | `x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture` |

### 3.2 How each permission is queried — `PermissionsManager.status(_:)`

| Permission | Query | Mapping |
|---|---|---|
| `.microphone` | `AVCaptureDevice.authorizationStatus(for: .audio)` | `.authorized`→`.granted`; `.notDetermined`→`.notDetermined`; `.denied`, `.restricted`, `@unknown default`→`.denied` |
| `.inputMonitoring` | **`IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`** | `kIOHIDAccessTypeGranted`→`.granted`; `kIOHIDAccessTypeDenied`→`.denied`; anything else→`.notDetermined` |
| `.accessibility` | `AXIsProcessTrusted()` | `true`→`.granted`, `false`→**`.notDetermined`** (comment: "macOS conflates not-asked and denied here. Treating both as 'needs action' is fine for the onboarding gate.") |
| `.screenRecording` | `CGPreflightScreenCaptureAccess()` | `true`→`.granted`, `false`→`.notDetermined` |

Note `.accessibility` and `.screenRecording` **can never return `.denied`**. That
matters: `requestAccess` only opens System Settings when `currentStatus == .denied`, so
for those two it always takes the prompt path first.

### 3.3 Gating helpers

```swift
static func allRequiredGranted() -> Bool {
    var snapshot: [Permission: PermissionStatus] = [:]
    for p in Permission.allCases { snapshot[p] = status(p) }
    return allRequiredGranted(from: snapshot)
}
static func allRequiredGranted(from snapshot: [Permission: PermissionStatus]) -> Bool {
    Permission.allCases.filter { $0.isRequired }.allSatisfy { snapshot[$0] == .granted }
}
```

The snapshot-taking variant exists so the gating decision is testable without touching
real TCC state (stated in the file header comment).

### 3.4 Requesting — `requestAccess(_:currentStatus:)`

```
if currentStatus == .denied { openSystemSettings(p); return }
switch p {
case .microphone:       AVCaptureDevice.requestAccess(for: .audio) { _ in }
case .inputMonitoring:  _ = IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)
                        openSystemSettings(.inputMonitoring)
case .accessibility:    _ = AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary)
                        openSystemSettings(.accessibility)
case .screenRecording:  _ = CGRequestScreenCaptureAccess()
}
```

**Input Monitoring and Accessibility both prompt AND open System Settings** — the OS
prompt for these only deep-links, so the pane is opened unconditionally. Microphone and
Screen Recording only prompt. `openSystemSettings(_:)` is
`NSWorkspace.shared.open(p.systemSettingsURL)`.

### 3.5 `PermissionStatusPoller`

```swift
final class PermissionStatusPoller: ObservableObject {
    @Published private(set) var statuses: [Permission: PermissionStatus] = [:]
    private var timer: Timer?
    init() {
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in self?.refresh() }
    }
    deinit { timer?.invalidate() }
    func refresh() {
        var next: [Permission: PermissionStatus] = [:]
        for p in Permission.allCases { next[p] = PermissionsManager.status(p) }
        if next != statuses { statuses = next }   // publish only on change
    }
    var allRequiredGranted: Bool { PermissionsManager.allRequiredGranted(from: statuses) }
}
```

**1.0 s interval, publish-on-change only**, because "macOS doesn't notify on TCC grants".
This is what makes the wizard's Grant buttons flip to ✓ without user action.

### 3.6 How it drives the onboarding wizard

`UI/OnboardingWindow.swift` (440 new lines) — a 480×600 SwiftUI window controller,
`OnboardingWindowController(settings:onCompleted:)`. Three paged steps (B-016 / B-017),
with step dots and Back / Continue / Finish setup buttons:

1. **Permissions.** One row per `Permission.allCases` — bolt icon header, title,
   rationale, live status from the poller, and a Grant button calling
   `PermissionsManager.requestAccess(p, currentStatus: poller.statuses[p])`.
   `poller.allRequiredGranted` gates Continue. Screen Recording is shown but never blocks.
2. **Mic test** (B-017). Spins a temporary `AudioRecorder` using the **shared**
   `AppSettings` instance (item 1 of `01a1f4c` — it previously called `AppSettings.load()`
   and created a parallel instance), displays live RMS via the same
   `onLevelUpdate` callback the pill uses, and switches its hint from `No signal yet` to
   `Looks good` once the level crosses a small threshold. Stops the recorder on disappear.
   Guarded by `AudioRecorder.isAnyActive` (B-028) — if a dictation is in flight it shows
   `A dictation is in progress — skip this step and test the mic after.` instead of
   opening a second `AVAudioEngine` on the same input.
3. **Vendor pick** (B-016). Radio picker writing `settings.activeVendor` immediately.
   Sign-in still happens in Settings → Accounts, linked from the finish CTA.

**Auto-show rule**, in `applicationDidFinishLaunching`:

```swift
let requiredOk = PermissionsManager.allRequiredGranted()
if !requiredOk || !settings.didCompleteOnboarding {
    showOnboarding()
}
wLog("Permissions on launch — mic=\(PermissionsManager.status(.microphone)) input=\(PermissionsManager.status(.inputMonitoring)) ax=\(PermissionsManager.status(.accessibility)) screen=\(PermissionsManager.status(.screenRecording))")
```

`showOnboarding()` lazily constructs the controller and nils it in the `onCompleted`
callback (which also logs `"Onboarding completed"`). It is **dismissible** — the window
can be closed with permissions still missing.

**Re-entry:** status-bar item `Setup & Permissions…` → `onShowOnboarding` →
`AppDelegate.showOnboarding()`.

**Mid-session revocation (B-029):** `StatusBarController` runs its **own** 30 s poll
(`permissionPollTimer`), separate from the wizard's 1 s poller, comparing against
`lastPermissionSnapshot`. On any change it rebuilds the menu. When
`hasPermissionRegression` (any required permission `!= .granted`), the menu pins a red
item at the top:

```swift
NSMenuItem(title: "⚠ A required permission was revoked", action: #selector(showOnboardingWindow), keyEquivalent: "")
// attributedTitle foregroundColor: NSColor.systemRed
```

and `refreshStatusIcon()` swaps the menu-bar icon for the badged variant (§8.1).

### What the Rust port must change (§3)

1. `MacPermissions::status(Permission::InputMonitoring)` currently uses
   `CGPreflightListenEventAccess()`. The Swift app uses **`IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`**
   with a three-way mapping including an explicit `Denied`. Either match it or document
   the deviation — `CGPreflightListenEventAccess` is boolean and cannot distinguish
   not-determined from denied, which changes whether `request()` prompts or deep-links.
2. `Permission::Accessibility` and `Permission::ScreenRecording` must map "not granted" to
   **`NotDetermined`**, never `Denied`, so the request path always prompts first.
3. `request()` for Input Monitoring and Accessibility must **also** open the Privacy pane
   after prompting. The port's `request()` only prompts.
4. Add `is_required` (mic / input monitoring / accessibility = true, screen recording =
   false), `title`, and the four verbatim `rationale` strings.
5. Add `all_required_granted(from: snapshot)` taking a snapshot map, so gating is testable.
6. **Build the onboarding wizard.** There is none in `ui/` — grep for `onboarding` across
   `ui/`, `src-tauri/src/`, `crates/` returns nothing. Three paged steps, the auto-show
   rule `!all_required_granted() || !did_complete_onboarding`, dismissibility, and the
   `Setup & Permissions…` tray re-entry.
7. Add a **1 s publish-on-change poller** for the wizard and a **30 s drift poll** for the
   tray, with the red `⚠ A required permission was revoked` pinned item and the badged
   tray icon.
8. Add `AudioRecorder::is_any_active()` (process-wide counter) so the mic-test step cannot
   open a second capture device during a live dictation.

---

## 4. `SafeCompletion<Value>` (`8a81d74`)

New file `Sources/WisprLightning/Services/SafeCompletion.swift`, 51 lines.

### 4.1 The hazard it prevents

**Double completion.** Several code paths race to complete a single async operation, and
before this type each site rolled its own `NSLock` + `Bool` + closure guard. The header
comment names the four ad-hoc copies it replaced: `WisprFlowProvider`,
`DeepgramProvider`, `AppDelegate.attemptTranscription`, `AppDelegate.autoPolishText`.

The concrete failure modes:

- **`attemptTranscription`** — the per-provider watchdog fires `.failure(.timeout)` at the
  same moment the provider's real completion arrives. Without a gate, chain advancement
  runs twice: `currentChainIndex` skips a step, two providers are rebuilt, two
  `attemptTranscription()` calls race through the `isTranscribing` guard, and the pill
  ends in an indeterminate state.
- **`autoPolishText`** — the 30 s watchdog injects the original text while the polish
  response injects the polished text. Without a gate the user gets **both**, pasted twice.

### 4.2 Exact semantics

```swift
final class SafeCompletion<Value> {
    private let lock = NSLock()
    private var hasFired = false
    private var body: ((Value) -> Void)?

    init(_ body: @escaping (Value) -> Void) { self.body = body }

    func fire(_ value: Value) {
        lock.lock()
        let runBody: ((Value) -> Void)?
        if hasFired { runBody = nil }
        else { hasFired = true; runBody = body; body = nil }
        lock.unlock()
        runBody?(value)
    }

    var hasCompleted: Bool {
        lock.lock(); defer { lock.unlock() }
        return hasFired
    }
}
```

Contract, point by point:

1. **At most one execution of `body`, ever.** First `fire` wins; every later `fire` is
   silently dropped — no error, no log, no return value.
2. **Runs on whatever thread fired first.** No queue hop is performed.
3. **`body` runs OUTSIDE the lock.** Documented reason: "so it can do arbitrary work
   (call into UI, schedule timers, etc.) without risking re-entrant deadlock". A `fire`
   from inside `body` therefore does not deadlock — it simply no-ops.
4. **The body reference is dropped at fire time** (`body = nil` under the lock), so
   captured state is released even if callers keep the gate alive. This is load-bearing:
   the transcription gate captures `[weak self]` plus `appInfo`, and the auto-polish gate
   captures the full transcript string.
5. `hasCompleted` is a lock-protected read, provided for callers that want to skip work.
   Nothing in the current tree reads it.
6. Generic over `Value`; the two in-tree instantiations are
   `SafeCompletion<Result<TranscriptResult, TranscriptionError>>` and
   `SafeCompletion<(text: String, isPolished: Bool)>`.

### 4.3 The two non-provider call sites, verbatim shape

`attemptTranscription()`:

```swift
let gate = SafeCompletion<Result<TranscriptResult, TranscriptionError>> { [weak self] result in
    guard let self = self else { return }
    self.handleTranscriptionResult(result, appInfo: appInfo)
}
// watchdog → gate.fire(.failure(.timeout))
// provider → watchdog.cancel(); gate.fire(result)
```

`handleTranscriptionResult(_:appInfo:)` was **extracted specifically** so both the
watchdog and the natural completion share one body.

`autoPolishText(_:)`:

```swift
private static let autoPolishWatchdogSeconds: TimeInterval = 30

let gate = SafeCompletion<(text: String, isPolished: Bool)> { [weak self] outcome in
    guard let self = self else { return }
    DispatchQueue.main.async {
        self.recordingOverlay.showInserting()
        self.textInjector.inject(text: outcome.text) { _ in
            DispatchQueue.main.async { self.recordingOverlay.hide() }
        }
        if outcome.isPolished { wLog("Auto-polish complete: \(outcome.text.count) chars") }
    }
}
let watchdog = DispatchWorkItem {
    wLog("Auto-polish watchdog fired — injecting original text")
    gate.fire((text: text, isPolished: false))
}
DispatchQueue.global().asyncAfter(deadline: .now() + 30, execute: watchdog)
// polish success → watchdog.cancel(); gate.fire((polishResult.polishedText, true)); polishStore.saveResult(...)
// polish failure → watchdog.cancel(); log; gate.fire((text, false))
```

Note the watchdog `DispatchWorkItem` is **cancelled** on the normal path *in addition to*
the gate — belt and braces, because `DispatchWorkItem.cancel()` after the deadline is a
no-op.

### What the Rust port must change (§4)

1. Add a `SafeCompletion<T>`-equivalent primitive. In Rust the natural shapes are a
   `tokio::sync::oneshot` sender wrapped so extra sends are dropped, or an
   `Once`+`Mutex<Option<Box<dyn FnOnce(T)>>>`. It must preserve: at-most-once, no queue
   hop, body executed with the lock released, and **the boxed closure dropped at fire
   time**.
2. `run_transcription` currently relies on `tokio::time::timeout` around the *whole*
   attempt sequence. Once the per-provider watchdog from §2.5 exists, its cancel path and
   the provider's own completion must funnel through one gate.
3. `auto_polish_text` (`src-tauri/src/pipeline/polish.rs`) needs the **30 s watchdog that
   injects the original transcript** on expiry, gated so the polished and original text
   can never both be injected.
4. Audit `wl-providers` for hand-rolled once-guards and replace them with the shared
   primitive — the Swift commit's whole point was that four divergent copies had drifted.

---

## 5. `TelemetryStore` (`8a81d74`)

New file `Sources/WisprLightning/Services/TelemetryStore.swift`, 73 lines.

### 5.1 What is recorded

```swift
struct AttemptRecord: Identifiable {
    let id: UUID
    let timestamp: Date
    /// Display name of the vendor that produced the final text, or nil if
    /// no provider succeeded.
    let finalVendor: String?
    /// 0 = primary vendor returned text. 1 = first fallback hop. etc.
    let fallbackHops: Int
    /// Whether the per-provider watchdog timer fired during this attempt
    /// (means at least one provider hung past its budget).
    let watchdogFired: Bool
    let elapsedSeconds: Double
    let outcome: Outcome
    /// First ~60 chars of the transcript on success; the error message on
    /// failure; nil on cancel.
    let preview: String?

    enum Outcome { case success, failure, cancelled }

    var symbol: String {
        switch outcome {
        case .success:   return "✓"
        case .failure:   return "✗"
        case .cancelled: return "⊘"
        }
    }
}
```

One record per **user-visible dictation attempt**, not per provider hop — `fallbackHops`
collapses the hops into a count.

### 5.2 Where it is stored

```swift
final class TelemetryStore {
    private let lock = NSLock()
    private var records: [AttemptRecord] = []
    private let maxRecords: Int
    init(maxRecords: Int = 10) { self.maxRecords = maxRecords }

    func record(_ record: AttemptRecord) {
        lock.lock()
        records.insert(record, at: 0)                                   // newest first
        if records.count > maxRecords { records.removeLast(records.count - maxRecords) }
        lock.unlock()
        DispatchQueue.main.async {
            NotificationCenter.default.post(name: .telemetryUpdated, object: nil)
        }
    }

    func recent() -> [AttemptRecord] { lock.lock(); defer { lock.unlock() }; return records }
}

extension Notification.Name {
    static let telemetryUpdated = Notification.Name("WisprLightningTelemetryUpdated")
}
```

**In memory only.** A bounded ring buffer of **10** entries, newest-first, `NSLock`-guarded
because writers are AppDelegate completion paths on arbitrary queues and the reader is the
main-thread menu build. Instantiated as a `let telemetryStore = TelemetryStore()` stored
property on `AppDelegate` — **it is never written to disk, never serialized, and dies with
the process.**

### 5.3 Does it leave the machine?

**No.** There is no network code in the file, no callers outside `AppDelegate` (writes) and
`StatusBarController` (reads), and no serialization. The `shareUsageData` setting is not
consulted because there is nothing to share. The header comment frames it purely as
observability for the user:

> Shown in the status-bar submenu so the user can see at a glance whether the fallback
> chain / watchdog / retry machinery is doing anything in practice — without the safety
> nets being visible, "the system works" and "the system silently lost a fallback hop"
> look identical until the next incident.

### 5.4 Who writes records

`AppDelegate.recordAttempt(outcome:vendor:preview:)`:

```swift
private func recordAttempt(outcome: AttemptRecord.Outcome, vendor: String?, preview: String?) {
    let started = attemptStartedAt ?? Date()
    let elapsed = Date().timeIntervalSince(started)
    let record = AttemptRecord(
        id: UUID(), timestamp: Date(),
        finalVendor: outcome == .success ? vendor : nil,
        fallbackHops: currentChainIndex,
        watchdogFired: attemptWatchdogFired,
        elapsedSeconds: elapsed,
        outcome: outcome, preview: preview
    )
    telemetryStore.record(record)
    attemptStartedAt = nil
    attemptWatchdogFired = false
}
```

`finalVendor` is **forced to `nil` unless the outcome is `.success`**, even when a vendor
name is passed in.

Accumulators live on `AppDelegate`:

- `attemptStartedAt: Date?` — set in `stopRecordingSession()` right after
  `currentRetryAttempt = 0`, i.e. after audio capture stops and before
  `dictationProvider.stop`.
- `attemptWatchdogFired: Bool` — set `false` alongside it, flipped `true` inside the
  watchdog `DispatchWorkItem`. **Sticky across chain hops and auto-retries** so the final
  record reflects the whole attempt.

Four call sites:

| Site | Outcome | Vendor | Preview |
|---|---|---|---|
| `cancelActiveRecording()` — only `if attemptStartedAt != nil` | `.cancelled` | `nil` | `nil` |
| `handleTranscriptionResult` success | `preview?.isEmpty == false ? .success : .failure` | `activeVendorForChainStep().displayName` | first 60 chars of `formattedText ?? asrText` |
| retries-exhausted branch | `.failure` | `activeVendorForChainStep().displayName` | `error.userMessage` |
| non-retryable branch | `.failure` | `activeVendorForChainStep().displayName` | `error.userMessage` |

Crucially, all of these run **before** `clearPendingTranscription()` resets
`currentChainIndex` / `attemptStartedAt`. The success path has an explicit comment:
"Record telemetry BEFORE clearPendingTranscription nukes currentChainIndex / attemptStartedAt."

### 5.5 The "Recent dictations" submenu

Built in `StatusBarController.buildMenu()`, immediately after the `Undo last dictation`
item, only when `!recents.isEmpty`. Parent item title: **`Recent dictations`**.

```swift
private static func makeTelemetryItem(_ record: AttemptRecord) -> NSMenuItem {
    let timeFmt = DateFormatter(); timeFmt.timeStyle = .short
    let timestamp = timeFmt.string(from: record.timestamp)

    var pieces: [String] = [record.symbol]
    if let vendor = record.finalVendor { pieces.append(vendor) }
    if record.fallbackHops > 0 { pieces.append("(+\(record.fallbackHops) hops)") }
    if record.watchdogFired { pieces.append("⏱") }
    pieces.append("• \(String(format: "%.1fs", record.elapsedSeconds))")
    pieces.append("  \(timestamp)")
    let item = NSMenuItem(title: pieces.joined(separator: " "), action: nil, keyEquivalent: "")
    item.isEnabled = false
    if let preview = record.preview, !preview.isEmpty { item.toolTip = preview }
    return item
}
```

Title shape, from the doc comment: `"✓ Deepgram • 2.4s  3:45 PM"`. Note the joiner is a
single space and the timestamp piece already carries two leading spaces. Items are
**disabled** (informational only); the preview/error text is the `toolTip`.

`StatusBarController` observes `.telemetryUpdated` on `.main` and calls `buildMenu()`;
the observer token is removed in `deinit`.

### What the Rust port must change (§5)

1. Add an in-memory, lock-guarded, newest-first ring buffer of **10** `AttemptRecord`s
   with exactly the seven fields above and the `✓` / `✗` / `⊘` symbols. Do **not**
   persist it and do **not** transmit it.
2. Thread `attempt_started_at` and `watchdog_fired` through the pipeline as sticky
   per-dictation accumulators that survive retries and chain hops, and record **before**
   pipeline state is reset.
3. Force `final_vendor` to `None` on any non-success outcome.
4. Add the **`Recent dictations`** tray submenu with the exact title format
   (`<symbol> <vendor> (+N hops) ⏱ • <elapsed>s  <short time>`), disabled items, and the
   preview/error as a tooltip. Rebuild the tray on a telemetry-changed signal.
5. Emit a `.cancelled` record from the pill-✕ / abort path, but only when an attempt was
   actually in flight.

---

## 6. The lifecycle refactor (`8a81d74`, with B-031 from `97bc7e7` and items 1 & 3 of `299f8b2`)

### 6.1 What moved

| Was | Is |
|---|---|
| `activeRecordingFileHandle`, `activeRecordingFileURL`, `pendingAudioFileURL`, `recordingIOQueue` (four `AppDelegate` ivars) | one `pendingAudio: RecordingArtifact?` |
| `clearPendingTranscription(deleteAudio: Bool = true)` | `clearPendingTranscription()` with **no** file-lifecycle responsibility; each caller deletes explicitly |
| Four ad-hoc `NSLock`+`Bool` completion guards | `SafeCompletion<Value>` |
| Inline transcription-completion closure | extracted `handleTranscriptionResult(_:appInfo:)`, shared by provider + watchdog |
| Three separate vendor lookups (`activeVendorForChainStep`, `providerForCurrentChainStep`, `advanceChainStep`) | single `vendorAtChainStep(_ index: Int)` helper; the other three delegate to it |
| Inline RIFF builder in `saveAudioToDownloads` | `AudioEncoding.wavData(from:)` (`0eb2570`) |
| `saveAudioToDisk(_:)` called from a background queue at stop | dead code; incremental `RecordingArtifact.append` at capture time |
| `logFile?.closeFile()` at terminate | `logQueue.sync { try? logFile?.close(); logFile = nil }` |
| `wLog` using `seekToEndOfFile()` / `write(_:)` (NSException, uncatchable) | `try handle.seekToEnd()` / `try handle.write(contentsOf:)`, nils the handle on error |
| No log size bound | 5 MB cap with one rotation (item 1 of `299f8b2`) |

### 6.2 `wLog` and log rotation

```swift
private let logFilePath   = ~/Library/Logs/WisprLightning.log
private let logRotatedPath = ~/Library/Logs/WisprLightning.log.1
private let logMaxBytes: UInt64 = 5 * 1024 * 1024
private var logFile: FileHandle? = { createFile(atPath: logFilePath, contents: nil); return FileHandle(forWritingAtPath:) }()
private let logQueue = DispatchQueue(label: "com.wisprlightning.log")
private var logBytesWritten: UInt64 = (attributesOfItem(logFilePath)[.size] as? UInt64) ?? 0
```

`wLog(_:)` body: `logQueue.async { … }` writing `"[<ISO8601>] <message>\n"` as UTF-8, then
**always** `NSLog("Wispr Lightning: %@", message)` outside the queue. Inside:

```swift
guard let handle = logFile else { return }
do {
    try handle.seekToEnd()
    try handle.write(contentsOf: data)
    rotateLogIfNeeded(addedBytes: data.count)
} catch {
    logFile = nil
    NSLog("Wispr Lightning: log write failed (%@); further log lines will go to NSLog only", error.localizedDescription)
}
```

`rotateLogIfNeeded(addedBytes:)`: `logBytesWritten &+= UInt64(addedBytes)`; when it
exceeds 5 MB — `try? logFile?.close()` (the **throwing** variant, deliberately, because
`closeFile()` raises an uncatchable NSException on a dead descriptor), remove
`.log.1`, move `.log` → `.log.1`, create a fresh `.log`, reopen the handle, reset the
counter. **One rotation is kept.**

`wLogVerbose(_:)` is unchanged: gated on the global `isVerboseLoggingEnabled`, prefixes
`[VERBOSE] `.

### 6.3 Startup sequence — `applicationDidFinishLaunching`, in order

1. `settings = AppSettings.load()`
2. `session = Session()`
3. `dbManager = DatabaseManager()`
4. `historyStore = HistoryStore(dbManager:)` — its `createTable()` calls
   `dbManager.migrate([...])`, which runs **synchronously** under the DB serial queue
5. `dictionaryStore`, `polishStore`, `notesStore` (all `DatabaseManager`-backed)
6. `audioRecorder = AudioRecorder(settings:)`
7. `activeVendor = DictationVendor(rawValue: settings.activeVendor) ?? .wisprFlow`
8. `dictationProvider = Self.makeProvider(vendor: activeVendor, session:settings:)`;
   `dictationProvider.dictionaryStore = dictionaryStore`
9. `polishService`, `textInjector = TextInjector(settings:)`, `soundManager`, `musicController`
10. `statusBarController = StatusBarController(session:settings:historyStore:dictionaryStore:notesStore:textInjector:telemetryStore:)`
11. `recordingOverlay = RecordingOverlay()`; `recordingOverlay.prewarm()`;
    `recordingOverlay.onCancelAction = { self?.cancelActiveRecording() }`; `toastNotification = ToastNotification()`
12. `isVerboseLoggingEnabled = settings.verboseLogging`
13. `settingsObserver` on `.settingsChanged` (main queue) → update
    `isVerboseLoggingEnabled`, `refreshProviderIfChanged()`, `rearmMicrophone()`
14. `audioDevicesObserver` on `.audioDevicesChanged` (main queue) → `updateMenu()`, then
    mid-recording-disconnect stop **or** `rearmMicrophone()`
15. `let hasSession = session.load()`
16. `startWisprFlowSessionWatcher()`
17. `statusBarController.updateMenu()`
18. `if !hasSession { statusBarController.openSettings() }`
19. `hotkeyListener = HotkeyListener(settings:session:currentVendor:onPress:onRelease:)`;
    `.onPolishPress = …`; `hotkeyListener.start()`
20. `statusBarController.onTogglePause`, `.onShowOnboarding`
21. `if settings.keepMicrophoneActive { audioRecorder.prewarm() }`
22. `DispatchQueue.global(qos: .utility).async` → `dictionaryStore.seedDefaults(userName: session.userFirstName)`,
    warm `getVocabularyPhrases()` / `getReplacements()` / `getSnippets()`, then
    `historyStore.prune()`
23. `NSWorkspace.shared.notificationCenter.addObserver(… willSleepNotification …)`
24. `if settings.showInDock { NSApp.setActivationPolicy(.regular) }`
25. build `NSApp.mainMenu` (About / Settings… ⌘, / Quit ⌘Q)
26. `cmdCommaMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown)` for ⌘,
27. onboarding gate + the four-permission log line (§3.6)
28. `wLog("Ready — press \(settings.hotkeyLabels.first ?? "Left Control") to start dictating")`
29. **`recoverPendingAudio()`**
30. `NSAppleEventManager.shared().setEventHandler(… kInternetEventClass / kAEGetURL …)`

**Ordering constraints, each load-bearing:**

- **(1) first.** Every other constructor takes `settings`.
- **(3) before (4)(5).** All four stores take the shared `DatabaseManager`.
- **(4) is synchronous.** `dbManager.migrate` uses `queue.sync`; the schema must be at the
  right `user_version` before any store issues a query.
- **(7) before (8).** `makeProvider` switches on `activeVendor`.
- **(8) before (10).** Not a compile dependency, but `refreshProviderIfChanged` (wired at
  13) compares against the stored `activeVendor`; if it were unset the first settings
  change would rebuild the provider spuriously.
- **(9) before (10).** `StatusBarController.init` now requires `textInjector`
  (for Undo) and `telemetryStore`.
- **(11) before (29).** `recoverPendingAudio` may call `recordingOverlay.showRetryableError`
  or `retryTranscription()` (which calls `recordingOverlay.showProcessing()`).
- **(15) before (19).** `HotkeyListener.rebuildHotkeySet()` (run by `start()`) calls
  `session.canUsePolish(activeVendor: currentVendor())`; without a loaded session the
  polish hotkey silently never registers.
- **(15) before (22).** `seedDefaults(userName: session.userFirstName)`.
- **(17) before (18).** The menu must exist before the settings window is opened from it.
- **(22) after (4).** `historyStore.prune()` issues SQL; it is deliberately deferred to a
  utility queue so launch is not blocked, but it must not race the migration — it does not,
  because `migrate` already completed synchronously at (4) and `prune` goes through
  `dbManager.sync`.
- **(27) before (29).** The onboarding window is shown before a recovery pill so the pill
  is not buried behind a modal-looking window.
- **(29) last-but-one.** Recovery needs the provider (8), the overlay (11), and the status
  bar (10) all live.
- **(30) after (2).** `handleURLEvent` writes into `session`.

### 6.4 Shutdown sequence — `applicationWillTerminate`, in order

1. `NotificationCenter.removeObserver(settingsObserver)`, `…(audioDevicesObserver)`
2. `NSWorkspace.shared.notificationCenter.removeObserver(self)`
3. `NSEvent.removeMonitor(cmdCommaMonitor)`
4. **(B-031)** invalidate + nil `recordingTimer`, `tapDelayTimer`,
   `processingTimeoutTimer`, `rearmTimer`
5. **(B-031)** `NSAppleEventManager.shared().removeEventHandler(forEventClass: kInternetEventClass, andEventID: kAEGetURL)`
6. `hotkeyListener.stop()`
7. `wisprFlowSessionWatcher?.cancel()`; `= nil`
8. **`pendingAudio?.finishWriting()`** — drain the incremental-write queue and close the
   handle so the partial `.pcm` on disk is valid and recoverable. **Never `delete()`.**
9. ```swift
   logQueue.sync {
       try? logFile?.close()
       logFile = nil
   }
   ```
10. `audioRecorder.cleanup()`
11. `historyStore.close()`
12. `dbManager.close()`

**Ordering constraints:**

- **(8) before (9)**, and **(9) before (10)–(12)**. The comment on (9) names the bug this
  fixed:

  > Drain any pending log writes on the serial log queue BEFORE closing the file.
  > Otherwise a wLog call in-flight from a background thread (audio, WS receive, settings
  > observer) lands on a closed FileHandle and abort()s the process during shutdown —
  > which was the root cause of the crash users saw on Cmd+Q after switching providers.

  Because step 9 sets `logFile = nil` under `logQueue.sync`, any `wLog` issued by steps
  10–12 hits `guard let handle = logFile else { return }` and degrades to `NSLog` only —
  no crash, but also no disk record of teardown.
- **(6) before (8).** Stopping the hotkey listener prevents a new recording starting
  between the timer teardown and the artifact close.
- **(4) before (8).** An un-invalidated `tapDelayTimer` could fire `stopRecordingSession()`
  mid-teardown.
- **(11) before (12).** `HistoryStore.close()` finalizes its statements against a handle
  `DatabaseManager.close()` is about to destroy.

### 6.5 Other lifecycle-adjacent changes in the same commit

- **`abortRecording(reason:)`** extracted as the shared teardown for non-graceful exits
  (system sleep, pill ✕). It clears `onLevelUpdate` / `onPacket`, discards packets via
  `_ = audioRecorder.stop()`, `pendingAudio?.finishWriting()` (keeps the file),
  `dictationProvider.cancel()`, `clearPendingTranscription()`,
  `statusBarController.setRecording(false)`, `recordingOverlay.hide()`,
  `resumeMusicInBackground()`, plus timer teardown and `hotkeyListener.resetState()`.
- **`cancelActiveRecording()`** — the pill ✕ handler. `guard isRecording else { return }`,
  log `"User cancelled recording via pill ✕"`, record a `.cancelled` telemetry entry only
  `if attemptStartedAt != nil`, then `abortRecording(reason: "user cancel")`.
- **`onSystemSleep()`** now routes through `abortRecording(reason: "system sleep")`
  instead of an inline teardown.
- **Force-unwrap fixes:** `AudioRecorder.processBuffer`'s `outputBuffer.int16ChannelData![0]`
  → `guard let int16Ptr = outputBuffer.int16ChannelData?[0] else { return }`;
  `FileManager.urls(...).first!` in both `pendingAudioDir` and `saveAudioToDownloads` →
  guarded with a home-directory fallback; `FileHandle.closeFile()` → `try? close()`.

### What the Rust port must change (§6)

1. **Adopt the startup order in §6.3 and its constraints.** The port's `setup()`
   (`src-tauri/src/lib.rs:189-334`) is close but differs materially: `check_permissions`
   runs at :323 with no onboarding gate, and `spool.recover_latest()` at :327 has no
   `< 90 s` auto-retry.
2. **Add the four-permission launch log line** with the exact `mic= input= ax= screen=`
   shape.
3. **Add log rotation**: 5 MB cap, one `.log.1` rotation, byte counter maintained
   in-process, handle nulled on write error with a one-time NSLog-equivalent notice.
4. **Drain the log writer before closing it at exit.** The port's
   `RunEvent::Exit` arm (`lib.rs:145-149`) stops the flow watcher and logs, then relies on
   drop order for everything else. Make the ordering explicit and put the artifact
   `finish_writing()` before the log drain.
5. **Retire the recording artifact explicitly at exit** — currently nothing flushes an
   in-flight recording on quit because nothing is being written during recording at all
   (§2).
6. **Extract a single `abort_recording(reason)`** shared by sleep and pill-✕, and route a
   `.cancelled` telemetry record through it.
7. Add a single `vendor_at_chain_step(index)` helper once the fallback chain lands, rather
   than three lookups.

---

## 7. `Settings` delta

### 7.1 Added fields (all in `AppSettings`, all `var`, all with the literal default shown)

| Field | Type | Default | Introduced by |
|---|---|---|---|
| `hotkeyTapToToggle` | `Bool` | `false` | B-011 `5ce7784` — **deprecated by B-015 but retained and kept in sync** |
| `hotkeyPressBehavior` | `String` | `"legacy"` | B-015 `d0370a5`; values `"hold"` \| `"toggle"` \| `"legacy"` |
| `activeVendor` | `String` | `DictationVendor.wisprFlow.rawValue` = **`"wispr_flow"`** | B-008 `2b87b70` |
| `openRouterModel` | `String` | `"google/gemini-2.5-flash-lite"` | B-008 `2b87b70` / `47fff75` |
| `fallbackChain` | `[FallbackStep]` | `[]` | B-012 `25b3315` |
| `deepgramLanguage` | `String` | `"en"` | `8a81d74` |
| `didCompleteOnboarding` | `Bool` | `false` | B-010 `776bf15` |

New nested type:

```swift
struct FallbackStep: Codable, Hashable, Identifiable {
    var id: UUID
    var vendor: String            // a DictationVendor rawValue
    var openRouterModel: String?  // honoured only when vendor == "openrouter"
    init(vendor: String, openRouterModel: String? = nil) {
        self.id = UUID(); self.vendor = vendor; self.openRouterModel = openRouterModel
    }
}
```

`deepgramLanguage` accepts a BCP-47 code (`"en"`, `"es"`, …) **or** one of two sentinels:
`"__auto__"` → `detect_language=true`, `"__multi__"` → `language=multi`.

`DictationVendor` raw values, for `activeVendor` and `FallbackStep.vendor`:
`"wispr_flow"`, `"openrouter"`, `"claude_voice"`, `"deepgram"`.

### 7.2 Removed fields

**None.** Nothing was removed from `AppSettings` between the two commits.

### 7.3 Renamed / re-annotated fields

**None renamed.** Two were re-annotated:

```swift
// Deprecated — kept for Codable backward-compat. All readers use the array form.
var hotkeyKeyCode: UInt16 = 59
var hotkeyLabel: String = "Left Control"
```

(was `// Left Ctrl (legacy single-key)` / `// legacy single-key`). See §1.8.

### 7.4 `load()` — rewritten (`.bak` = item 13 of `299f8b2`; atomic snapshot = item 3 of `b0b3899`)

```swift
static var backupURL: URL { settingsURL.appendingPathExtension("bak") }   // settings.json.bak

static func load() -> AppSettings {
    if let data = try? Data(contentsOf: settingsURL),
       let settings = try? JSONDecoder().decode(AppSettings.self, from: data) {
        let tmpURL = backupURL.appendingPathExtension("tmp")              // settings.json.bak.tmp
        try? FileManager.default.removeItem(at: tmpURL)
        do {
            try data.write(to: tmpURL, options: .atomic)
            _ = try? FileManager.default.replaceItemAt(backupURL, withItemAt: tmpURL)
        } catch { /* best-effort — leave any existing .bak in place */ }
        return applyMigrations(settings)
    }
    if let data = try? Data(contentsOf: backupURL),
       let settings = try? JSONDecoder().decode(AppSettings.self, from: data) {
        NSLog("Wispr Lightning: settings.json was unreadable; restored from .bak")
        return applyMigrations(settings)
    }
    let settings = AppSettings()
    settings.save()
    return settings
}
```

The `.bak` is a snapshot of the **just-validated primary**, written via a sibling `.tmp`
and `replaceItemAt` so there is never a window with no backup (`b0b3899` item 3 — the
prior shape was `removeItem` + `copyItem`).

```swift
private static func applyMigrations(_ settings: AppSettings) -> AppSettings {
    if settings.hotkeyKeyCodes.isEmpty && settings.hotkeyKeyCode != 0 {
        settings.hotkeyKeyCodes = [settings.hotkeyKeyCode]
        settings.hotkeyLabels = [settings.hotkeyLabel]
    }
    if settings.hotkeyPressBehavior.isEmpty {
        settings.hotkeyPressBehavior = settings.hotkeyTapToToggle ? "toggle" : "legacy"
    }
    return settings
}
```

> **Verified by experiment (`swiftc` on this machine, 2026-08-02):** Swift's *synthesized*
> `Codable` conformance emits `decode(_:forKey:)`, **not** `decodeIfPresent`, for
> non-optional stored properties — a property's default value is **not** used when the key
> is missing. Decoding `{"b": 7}` into `class S: Codable { var a: String = "legacy"; var b: Int = 5 }`
> throws `DecodingError.keyNotFound` for `a`.
>
> Two consequences for `AppSettings`:
> 1. **Both migration branches are dead in practice.** `hotkeyPressBehavior.isEmpty` can
>    only be true if the JSON literally contains `"hotkeyPressBehavior": ""`; a *missing*
>    key aborts the whole decode long before `applyMigrations` runs. Same for
>    `hotkeyKeyCodes: []` + `hotkeyKeyCode`.
> 2. **Every release that adds a non-optional field resets every existing user's
>    settings once.** The old `settings.json` fails to decode → `.bak` (equally old) fails
>    → `AppSettings()` defaults + `save()`. The seven fields in §7.1 each triggered this.
>    After one launch the file is complete again, so the reset is one-shot per release.

### 7.5 `save()` — rewritten (B-026 `97bc7e7`)

```swift
private static let saveQueue = DispatchQueue(label: "com.wisprlightning.settings.save")
private static let pendingSaveLock = NSLock()
private static var pendingSaveItem: DispatchWorkItem?

func save() {
    // 1. Notify UI observers immediately, on main, before any disk I/O.
    let postNotification: () -> Void = { [weak self] in
        guard let self else { return }
        NotificationCenter.default.post(name: .settingsChanged, object: self)
    }
    if Thread.isMainThread { postNotification() } else { DispatchQueue.main.async { postNotification() } }

    // 2. Snapshot on the calling thread so the deferred write never reads mutable state.
    let snapshot = self.encodedSnapshot()

    // 3. Debounce: only the last save in a 100 ms window reaches disk.
    Self.pendingSaveLock.lock()
    Self.pendingSaveItem?.cancel()
    let item = DispatchWorkItem {
        guard let data = snapshot else { return }
        try? data.write(to: Self.settingsURL, options: .atomic)
    }
    Self.pendingSaveItem = item
    Self.pendingSaveLock.unlock()
    Self.saveQueue.asyncAfter(deadline: .now() + 0.1, execute: item)
}

private func encodedSnapshot() -> Data? {
    guard let data = try? JSONEncoder().encode(self) else { return nil }
    if let json = try? JSONSerialization.jsonObject(with: data),
       let pretty = try? JSONSerialization.data(withJSONObject: json, options: .prettyPrinted) {
        return pretty
    }
    return data
}
```

Four changes from baseline: **notification first** (was last), **snapshot on the caller's
thread**, **100 ms debounce on a serial queue**, and **`.atomic` write** (was a plain
`write(to:)`). The queue/lock/work-item are `static` so synthesized `Codable` conformance
does not trip over a non-`Codable` `DispatchWorkItem` stored property.

Related: `exportSettings` now encodes the **live instance** rather than reading
`settings.json` from disk (`b0b3899` item 1) — with the 100 ms debounce, a toggle
immediately followed by Export would otherwise export the stale file.
`importSettings` writes to disk then `open -n <bundle>` + `NSApp.terminate(nil)`
(§8.7).

### 7.6 Migration-break matrix against the Rust port

`crates/wl-core/src/settings.rs` uses `#[serde(default)]` on the struct, so **missing keys
fall back individually and unknown keys are ignored** — the port already fixed the Swift
cliff-edge (DV3 in its own doc comment). But serde **does not round-trip unknown keys**:
anything not declared in `Settings` is dropped on the next `save()`.

| Swift field | Rust field | Break |
|---|---|---|
| `activeVendor: String` = `"wispr_flow"` | `provider: ProviderId` under key **`"provider"`**, values `"wispr"` / `"deepgram"` | **BREAK — key name *and* value vocabulary both differ.** Swift `"deepgram"` ≠ Rust `"deepgram"` only by luck of the enum; `"wispr_flow"` vs `"wispr"` does not match. Each side falls back to its own default and the other's key is dropped on save. |
| `fallbackChain: [FallbackStep]` | *(absent)* | **BREAK — destroyed on any Rust save.** The whole multi-vendor fallback feature. |
| `openRouterModel: String` | *(absent)* | **BREAK — destroyed on any Rust save.** |
| `deepgramLanguage: String` = `"en"` | *(absent)*; Rust instead has `deepgramModel`, `deepgramKeytermBoost`, `localPostProcessing`, which **Swift does not have** | **BREAK both directions.** Deepgram config is disjoint. |
| `didCompleteOnboarding: Bool` | *(absent)* | **BREAK — destroyed on any Rust save.** The wizard would re-show every launch once implemented. |
| `hotkeyTapToToggle: Bool` | *(absent)* | **BREAK — destroyed on any Rust save.** |
| `hotkeyPressBehavior: String` = `"legacy"` | *(absent)* | **BREAK — destroyed on any Rust save.** Press behaviour silently reverts to legacy. |
| `hotkeyKeyCode: UInt16`, `hotkeyLabel: String` | *(absent)* | **BREAK.** Rust never writes them; a Swift build then fails to decode the file entirely (§7.4) and resets everything. |
| `hotkeyKeyCodes: [UInt16]` | `legacy_hotkey_key_codes` (`rename = "hotkeyKeyCodes"`), **cleared after migration** | **BREAK.** Rust writes `"hotkeyKeyCodes": []`; Swift's `rebuildHotkeySet` then falls back to `[59]`, silently discarding a custom binding. Same for `polishHotkeyKeyCodes`. |
| `hotkeyLabels: [String]`, `polishHotkeyLabels: [String]` | *(absent)* | **BREAK — destroyed on any Rust save.** The Settings UI loses its key labels. |
| `micDeviceUID: String?` | `mic_device_id` under key **`"micDeviceId"`** | **BREAK.** Deliberate port change (the value format also gains a `coreaudio:` / `wasapi:` prefix), but it means mic selection is lost in both directions. |
| `naturalModeSpeed: String` (`"slow"`/`"normal"`/`"expert"`) | `TypingSpeed` enum, `rename_all = "lowercase"` | Compatible. |
| `emailSignatureOption: String` = `"written_with_lightning"` | `EmailSignature`, `rename_all = "snake_case"` | Compatible. |
| `hotkeyPaused`, `keepMicrophoneActive`, `enableSounds`, `muteMusic`, `selectedSoundPack`, `languages`, `aiFormatting`, `autoCleanupLevel`, `commandModeEnabled`, `useScreenContext`, `useAccessibilityContext`, `styleDetectionEnabled`, `personalizationStyles`, `hyperlinkOn`, `creatorMode`, `autoLearnWords`, `polishEnabled`, `polishInstructions`, `autoPolish`, `naturalModeEnabled`, `emailAutoSignature`, `launchAtLogin`, `showInDock`, `shareUsageData`, `verboseLogging`, `micDeviceName` | present, same key | Compatible. |

Also divergent, outside the field list:

- **Backup strategy.** Swift writes `settings.json.bak` on every successful load and falls
  back to it. Rust renames a bad file to `settings.json.corrupt` and returns defaults —
  it never keeps a known-good copy, so the first corruption is unrecoverable.
- **Debounce.** Swift coalesces saves in a 100 ms window on a serial queue. Rust's
  `Settings::save` writes synchronously (tmp + rename) on the calling thread every time.
- **Notification timing.** Swift posts `.settingsChanged` before the write; the port must
  not couple its equivalent signal to disk completion.

### What the Rust port must change (§7)

1. **Rename `provider` back to `activeVendor`** and use the Swift raw values
   (`"wispr_flow"`, `"openrouter"`, `"claude_voice"`, `"deepgram"`). Keep a
   `#[serde(alias = "provider")]` if a port-written file must still load, but the canonical
   key is `activeVendor`.
2. **Add every missing field with its exact default:** `hotkeyTapToToggle: bool = false`,
   `hotkeyPressBehavior: String = "legacy"`, `openRouterModel: String = "google/gemini-2.5-flash-lite"`,
   `fallbackChain: Vec<FallbackStep> = []`, `deepgramLanguage: String = "en"`,
   `didCompleteOnboarding: bool = false`, plus `hotkeyKeyCode: u16 = 59`,
   `hotkeyLabel: String = "Left Control"`, `hotkeyLabels: Vec<String> = ["Left Control"]`,
   `polishHotkeyLabels: Vec<String> = ["Right Control"]`. Add `FallbackStep { id: Uuid,
   vendor: String, openRouterModel: Option<String> }`.
3. **Stop clearing `hotkeyKeyCodes` / `polishHotkeyKeyCodes` after migration.** Write them
   back so a Swift build (or a rollback) still sees the binding. If the port genuinely
   wants portable hotkeys as the source of truth, it must keep the Carbon arrays as a
   mirrored projection, not a one-shot input.
4. **Decide `micDeviceUID` vs `micDeviceId`.** If the prefixed format stays, add a
   migration that reads `micDeviceUID` and writes `micDeviceId` on first load, and keep
   writing both until the Swift app is retired.
5. **Reconcile Deepgram settings.** Swift has only `deepgramLanguage` (with the
   `"__auto__"` / `"__multi__"` sentinels). Rust has three fields Swift does not know
   about. Either land `deepgramLanguage` in Rust and `deepgramModel` /
   `deepgramKeytermBoost` / `localPostProcessing` in Swift, or accept and document that
   Deepgram config does not round-trip.
6. **Preserve unknown keys on save.** Add a
   `#[serde(flatten)] extra: serde_json::Map<String, Value>` catch-all so a Rust save never
   silently deletes a field a newer Swift build wrote. This is the single highest-value
   change in this section — without it, running the port once against a real user's
   `settings.json` destroys their fallback chain, model choice, press behaviour and
   onboarding flag.
7. **Add the `.bak` snapshot-on-successful-load** and the `.bak` fallback path, written via
   tmp + atomic replace. Keep the existing `.json.corrupt` rename as a third tier.
8. **Debounce saves at 100 ms** on a serial writer, snapshotting the encoded bytes on the
   calling thread, and emit the settings-changed signal immediately rather than after the
   write.

---

## 8. Concurrency and race fixes from the `/simplify` commits

Each is a hazard the Rust port may share. Ordered by the commit that fixed it.

### 8.1 DB queue race — `DatabaseManager` (B-025 `97bc7e7`, tightened by `8a345a7`)

**Hazard.** `DatabaseManager.transaction(_:)` issued `BEGIN` / `block()` / `COMMIT`
directly on the shared `sqlite3*` from whatever thread called it. Apple's libsqlite3 is
`SQLITE_THREADSAFE=1` (serialized handles) so it does not corrupt, but interleaved
transactions from two threads produce nested/aborted `BEGIN`s and unpredictable WAL
behaviour. `8a345a7` found a second instance: `userVersion` was an internal-access `var`
whose getter and setter both hit sqlite3 **outside** the queue, so any caller could race
the migration writer — "silent torn reads or, on a non-Apple SQLite build, an actual crash".

**Fix.**

```swift
let queue = DispatchQueue(label: "com.wisprlightning.db")

private func currentUserVersion() -> Int { /* PRAGMA user_version; must hold the queue */ }
private func setUserVersion(_ v: Int)   { /* PRAGMA user_version = v; must hold the queue */ }

func migrate(_ migrations: [String]) {
    queue.sync {
        let current = currentUserVersion()
        guard current < migrations.count else { return }
        for idx in current..<migrations.count {          // 2a88f2f: pending range, not enumerated()+filter
            exec("BEGIN TRANSACTION;")
            if sqlite3_exec(db, migrations[idx], nil, nil, nil) == SQLITE_OK {
                setUserVersion(idx + 1)
                exec("COMMIT;")
            } else {
                NSLog("Wispr Lightning: schema migration %d failed — %s", idx, String(cString: sqlite3_errmsg(db)))
                exec("ROLLBACK;")
                return
            }
        }
    }
}

func sync<T>(_ block: () -> T) -> T { return queue.sync(execute: block) }
func transaction(_ block: () -> Void) { queue.sync { exec("BEGIN TRANSACTION;"); block(); exec("COMMIT;") } }
```

`currentUserVersion` / `setUserVersion` are `private` precisely so nothing outside
`migrate` can reach them. **One transaction per migration step**, so partial progress
survives a crash.

`HistoryStore.createTable()` now expresses its schema through this pipeline:

```swift
dbManager.migrate([
    // v1 — initial schema (existing installs run this as a no-op because of IF NOT EXISTS).
    """
    CREATE TABLE IF NOT EXISTS transcripts (
        id TEXT PRIMARY KEY, asr_text TEXT, formatted_text TEXT, timestamp REAL,
        duration REAL, app_name TEXT, app_bundle_id TEXT, num_words INTEGER, language TEXT
    );
    """,
    // Future migration template:
    //   "ALTER TABLE transcripts ADD COLUMN provider TEXT;"
    // Append above this comment when adding columns.
])
```

with the explicit rule: *append to the END — never edit or reorder existing ones; the index
is the schema version baked into `PRAGMA user_version` on existing installs.*

**Port status.** `crates/wl-core/src/db/mod.rs` already has a `user_version` migration
runner (`migrate` at :233, transaction per step at :250) and holds the connection behind a
`Mutex`. This hazard is **already handled**. What is missing is `HistoryStore::prune` and
the raised vocabulary cap (§8.8).

### 8.2 Audio engine race — `AVAudioEngineConfigurationChange` (`fdec4d7`)

**Hazard.** The observer was registered with `queue: nil`, so its block ran on the
**posting** thread. Inside, it mutated `isPrewarmed` and called `audioEngine.stop()` /
`removeTap` — state also touched from main by `start()`, `stop()` and `deactivate()`.

**Fix.** The entire body now bounces onto main, and gained engine recovery (item 5 of
`299f8b2`):

```swift
engineConfigObserver = NotificationCenter.default.addObserver(
    forName: .AVAudioEngineConfigurationChange, object: audioEngine, queue: nil
) { [weak self] _ in
    guard let self = self else { return }
    NSLog("Wispr Lightning: AVAudioEngine configuration changed")
    self.invalidateDeviceCache()
    DispatchQueue.main.async {
        if self.isPrewarmed && !self.isRecording {
            self.audioEngine.inputNode.removeTap(onBus: 0)
            self.audioEngine.stop()
            self.isPrewarmed = false
            NSLog("Wispr Lightning: Audio engine force-reset after config change")
        }
        NotificationCenter.default.post(name: .audioDevicesChanged, object: nil)
    }
}
```

Note `invalidateDeviceCache()` stays off-main (it has its own `cacheLock`); only the
engine mutations and the notification post hop.

### 8.3 Sweep race — `sweepStalePendingAudio` (`fdec4d7`)

**Hazard.** The sweep read `pendingAudioFileURL` from a background queue while
`clearPendingTranscription` / `saveAudioToDisk` wrote it from main — an unsynchronized
cross-thread read of a `URL?` ivar.

**Fix.** Snapshot the active path **on main**, pass it in as a `String?`, and compare by
string equality inside the sweep — "which avoids holding the URL across queues".
Signature is now `sweepStalePendingAudio(activePath: String?)`. See §2.6 for the residual
ordering bug at the one call site.

### 8.4 `Session` race — torn reads and duplicate refreshes (`246ca4b` + B-024 `97bc7e7`)

**Hazard A — torn read.** `Session.isValid` read `accessToken` and `expiresAt` from main
while `refresh()`'s URLSession completion wrote them from a background queue. A read
interleaved between the two writes sees `accessToken` updated but `expiresAt` still `0`
(or vice versa) and "could flip the gate the wrong way".

**Hazard B — duplicate refresh.** Two near-simultaneous dictations on a just-expired token
both call `refresh()`, both POST to Supabase, both get token pairs, and one write silently
overwrites the other. With refresh-token rotation the loser's token is already revoked.

**Fix.** Two separate locks:

```swift
private let stateLock = NSLock()        // serializes token field reads/writes
private let refreshLock = NSLock()      // guards the in-flight flag + waiter queue
private var refreshInFlight = false
private var refreshWaiters: [(Bool) -> Void] = []
```

`isValid` takes `stateLock` for its whole body. `clear()` takes `stateLock` for its whole
body. `enrichFromJWT` was split into `enrichFromJWTLocked` ("Caller must hold `stateLock`")
so `refresh()` can wrap the token writes **and** the JWT enrichment in one critical
section without re-entering the lock:

```swift
self.stateLock.lock()
self.accessToken = newAccessToken
self.refreshToken = newRefreshToken
let absExpiry = json["expires_at"] as? TimeInterval ?? 0
let relExpiry = json["expires_in"] as? TimeInterval ?? 0
self.expiresAt = absExpiry > Date().timeIntervalSince1970
    ? absExpiry
    : (relExpiry > 0 ? Date().timeIntervalSince1970 + relExpiry : 0)
self.enrichFromJWTLocked(newAccessToken)
self.stateLock.unlock()
self.save()
```

Coalescing:

```swift
refreshLock.lock()
if refreshInFlight { refreshWaiters.append(completion); refreshLock.unlock(); return }
refreshInFlight = true
refreshLock.unlock()

let finish: (Bool) -> Void = { [weak self] success in
    guard let self else { completion(success); return }
    self.refreshLock.lock()
    let waiters = self.refreshWaiters
    self.refreshWaiters.removeAll()
    self.refreshInFlight = false
    self.refreshLock.unlock()
    completion(success)                 // the originating caller first
    for w in waiters { w(success) }     // then every piggybacked caller
}
```

Every `completion(false)` / `completion(true)` on the network path became `finish(false)` /
`finish(true)`. The early `guard accessToken != nil else { return false }`-style bail-outs
before the coalescing block still call `completion` directly — they never set the flag.

Also in `246ca4b`: `WisprFlowProvider` posted `.sessionChanged` from URLSession's
background queue while the status-bar observer rebuilding the menu lives on `.main`; the
post is now bounced through `DispatchQueue.main`.

**Port status.** `crates/wl-providers/src/session.rs` uses an `RwLock<Tokens>` and writes
all fields inside one `self.write()` scope (`session.rs:274-287`), so **Hazard A is
already handled**. `refresh()` has **no coalescing whatsoever** — Hazard B is live.

### 8.5 Chain re-prime — the fallback chain was completely broken (`0eb2570`)

**Hazard.** `attemptTranscription()` re-fed the buffered packets only when
`currentRetryAttempt > 0`. Every fallback chain step beyond the primary ran `stop()`
against a **freshly constructed, empty provider** and returned `emptyResult`. Since
`emptyResult.shouldFallback == false`, the chain then halted. The entire B-012 feature was
inert.

**Fix.**

```swift
// Re-prime the provider's internal buffer whenever we're talking to a
// provider that wasn't fed live during recording — that's the case
// for every retry (manual or auto), every fallback chain step beyond
// the primary, and after dismissRetry+retryTranscription. The initial
// attempt is skipped because audioRecorder.onPacket already fed it.
if currentRetryAttempt > 0 || currentChainIndex > 0 {
    dictationProvider.cancel()
    dictationProvider.start()
    for packet in packets { dictationProvider.feed(packet: packet) }
}
```

Paired fix in `retryTranscription()`: manual Retry now resets `currentChainIndex = 0` and
rebuilds the **primary** provider, so Retry restarts from step 1 instead of replaying
whatever vendor the chain happened to end on:

```swift
currentChainIndex = 0
currentRetryAttempt = 1
dictationProvider.cancel()
dictationProvider = Self.makeProvider(vendor: activeVendor, session: session, settings: settings)
dictationProvider.dictionaryStore = dictionaryStore
recordingOverlay.showProcessing()
dictationProvider.prewarmConnection()
scheduleProcessingTimeout()
DispatchQueue.global(qos: .userInitiated).async { [weak self] in self?.attemptTranscription() }
```

**The general hazard:** any streaming-provider design where the audio is fed live during
recording must explicitly replay the buffer for every attempt that is *not* the first.

### 8.6 Audio callback race — packets dropped at engine start (`0eb2570`)

**Hazard.** `audioRecorder.onPacket` / `onLevelUpdate` were assigned **after**
`audioRecorder.start()`. Packets emitted during the engine's startup tick hit a `nil`
callback and were "silently dropped, costing us the first few ms of audio".

**Fix.** Both callbacks are wired **before** `start()`, with an explicit comment saying so
(§2.3). And on the `.failed` path the just-wired callbacks are torn down so the next
dictation does not inherit stale closures:

```swift
case .failed(let reason):
    wLog("Failed to start recording: \(reason)")
    audioRecorder.onPacket = nil
    audioRecorder.onLevelUpdate = nil
    dictationProvider.cancel()
    recordingState = .idle
    recordingOverlay.showError(message: "Mic unavailable")
    musicController.resumeMusic()
    return
```

`stopRecordingSession()` and `abortRecording()` both nil the two callbacks before calling
`audioRecorder.stop()`, in that order.

### 8.7 Import data loss (`fdec4d7`, softened by `fb41b57`)

**Hazard.** `SettingsViewModel.importSettings` wrote the imported JSON to `settings.json`
but never copied the values into the live `AppSettings` instance. Any subsequent `save()` —
including one from the 100 ms debounce of an unrelated toggle — overwrote the import with
stale in-memory state. Silent, total loss of the imported file.

**Fix.** Write to disk, then relaunch so a fresh process reads the just-written file:
`Process` → `/usr/bin/open -n <bundle>` → `NSApp.terminate(nil)`. Confirmation alert
retitled **"Import & Relaunch"**. `fb41b57` item 3 then made the terminate conditional:

> Now: try, succeed → terminate; throw → log + show "Quit and reopen manually" alert with
> the running instance intact.

**Generalized hazard:** *any* import/restore path that writes the config file without
replacing the live in-memory model is a data-loss bug the moment saves are debounced.

### 8.8 Vocabulary truncation (item 2 of `01a1f4c`)

`DictionaryStore.getVocabularyPhrases(limit:)` default raised **50 → 500**, and a warning
was added when the fetch saturates:

```swift
if phrases.count >= limit {
    NSLog("Wispr Lightning: dictionary phrase fetch hit the limit (%d) — increase if you have more custom terms", limit)
}
```

Providers cap further at their own layer (Claude Voice keyterms takes the first 20; Nova-3
accepts up to 500 keyterm tokens). The cache (`cachedVocabulary`) is unchanged and is
still checked before the query.

**Port status.** `crates/wl-core/src/db/dictionary.rs:30` — `const VOCABULARY_LIMIT: i64 = 50;`.
**Stale.**

### 8.9 Clipboard restore race + everything else worth naming

**Clipboard race (item 4 of `299f8b2`).** `pasteViaClipboard` captures
`NSPasteboard.general.changeCount` **after** writing the transcript, and the deferred
restore aborts if the count has advanced:

```swift
var ourChangeCount: Int = 0
DispatchQueue.main.sync {
    let pasteboard = NSPasteboard.general
    pasteboard.clearContents()
    pasteboard.setString(text, forType: .string)
    ourChangeCount = pasteboard.changeCount
}
// … post Cmd+V …
DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
    let current = NSPasteboard.general.changeCount
    if current != ourChangeCount {
        wLog("Clipboard changed during paste (count \(ourChangeCount)→\(current)) — leaving user's value in place")
        return
    }
    Self.restoreClipboard(savedItems)
    if !savedItems.isEmpty { NSLog("Wispr Lightning: Clipboard restored (%d items)", savedItems.count) }
}
```

The restore delay is **0.25 s** and unchanged.

**`HotkeyListener` observer leak (B-032 `97bc7e7`).** The two `NotificationCenter`
observers (`.settingsChanged`, `.sessionChanged`) now capture their tokens into
`notificationObservers: [NSObjectProtocol]`, and a `deinit` calls `removeMonitors()` and
removes both.

**Wispr Flow watcher partial read (B-030 `97bc7e7`).**
`DispatchSourceFileSystemObject`'s `.write` fires mid-write, so `session.load()` can see
truncated JSON. `attemptWisprFlowSessionMigration(attempt:maxAttempts:)` retries up to
**5** times. The delay formula is `Double(attempt * attempt) * 0.05 + 0.05`, i.e.
**100 ms, 250 ms, 500 ms, 850 ms** (both the commit message's "50, 100, 250, 500, 850 ms"
and the source comment's "50, 150, 400, 900ms" are wrong — the code is authoritative).
Every retry re-checks `guard !self.session.isValid` before recursing, and the event handler
itself early-returns when a valid session already exists.

**`lockFocus` thread guard (`8a345a7`).** `StatusBarController.cachedAttentionIcon` is a
`static let` whose initializer calls `NSImage.lockFocus`, which asserts main thread. A
`precondition(Thread.isMainThread, "cachedAttentionIcon must be initialised on the main thread (lockFocus requirement)")`
was added so a future off-main refresh crashes at the initializer instead of deep inside
AppKit.

**Crash-report scan off the main thread (`fb41b57` item 1).** `buildMenu()` was doing
`~/Library/Logs/DiagnosticReports` directory I/O synchronously on every rebuild.
`cachedCrashReportsIfFresh()` now returns the cache and kicks a background refresh at most
once per **300 s**, rebuilding the menu only if the file set changed. The filename match
was also tightened from `lastPathComponent.lowercased().contains("wisprlightning")` to
`hasPrefix("WisprLightning-") || hasPrefix("WisprLightning_")` with extension `ips` or
`crash`, within 7 days, newest first, `prefix(2)` shown.

**`HistoryStore.prune` error surfacing (`fb41b57` item 2).** `sqlite3_step` /
`sqlite3_exec` return values are now checked and logged with `sqlite3_errmsg`, because "a
wedged DB … would silently skip prune every launch and the table would still grow
unbounded". `prune(olderThanDays: 180, cap: 10_000)` runs inside `dbManager.sync`, deleting
by `timestamp < cutoff` and then capping via
`DELETE FROM transcripts WHERE id NOT IN (SELECT id FROM transcripts ORDER BY timestamp DESC LIMIT <cap>)`.

**`AudioRecorder.isAnyActive` (B-028 `97bc7e7`).** A process-wide
`NSLock`-guarded counter, bumped `+1` in `start()` (unconditionally, before the prewarm
fast path), `−1` on the `.failed` return, and `−1` at the top of `stop()` only
`if isRecording`. `bumpActive` clamps with `max(0, activeCount + delta)`.

### What the Rust port must change (§8)

1. **8.1 DB queue** — already handled by `wl-core::db`'s `Mutex` + `user_version` runner.
   No change beyond adding `prune` (§8.9) and raising the vocab cap (§8.8).
2. **8.2 Audio engine race** — audit `crates/wl-platform/src/audio_impl.rs` for
   device/format-change callbacks that mutate prewarm state off the owning thread, and add
   the **force-reset when prewarmed-but-idle** behaviour after a config change.
3. **8.3 Sweep race** — when the 24 h sweep is added (§2), snapshot the active path
   *before* clearing pending state and compare by string.
4. **8.4 Session** — the torn read is already prevented by `RwLock`. **Add refresh
   coalescing:** an in-flight flag plus a waiter list (or a shared
   `tokio::sync::Mutex<Option<Shared<BoxFuture>>>`), so N concurrent `refresh()` calls
   produce one HTTP request and N identical results. Without it, refresh-token rotation
   will intermittently sign the user out.
5. **8.5 Chain re-prime** — when the fallback chain lands, every attempt with
   `retry_attempt > 0 || chain_index > 0` must cancel, restart and **replay the whole
   packet buffer** into the new provider before calling stop. Manual retry must reset the
   chain index to 0 and rebuild the primary provider.
6. **8.6 Audio callback race** — the port's `stop_recording` collects packets from a
   channel rather than a callback, so this specific shape does not apply; but once the
   incremental spool writer (§2) and level meter (§1.7) are wired, both **must** be
   installed before the capture stream starts, and torn down on the start-failure path.
7. **8.7 Import** — if a settings import/export feature is added, it must replace the live
   in-memory `Settings` (or relaunch), never just write the file.
8. **8.8 Vocabulary** — raise `VOCABULARY_LIMIT` from `50` to `500` and log on saturation.
9. **8.9 Clipboard** — `MacInjector::restore_after` uses a generation counter internal to
   the app. Add the **OS-level `NSPasteboard.changeCount` / Windows clipboard-sequence
   check** so a third-party clipboard manager's write is never stomped. Also add:
   `HistoryStore::prune(180 days, 10_000)` at launch with error logging; the crash-report
   tray items with the 300 s cache and the strict `WisprLightning-` / `WisprLightning_`
   prefix match; the version footer from the bundle; and
   `AudioRecorder::is_any_active()`.

---

## 9. Quick checklist for the port

Ordered by blast radius, highest first.

| # | Change | Section |
|---|---|---|
| 1 | Settings: `#[serde(flatten)] extra` catch-all so a Rust save cannot destroy Swift-only fields | §7 |
| 2 | Settings: `activeVendor` key + all seven new fields with exact defaults | §7 |
| 3 | Spool: write incrementally from recording start; delete only after a successful inject | §2 |
| 4 | Injector: delete paste verification | §1.1 |
| 5 | Injector: newline = Shift+Return | §1.3 |
| 6 | FSM: three press behaviours driven by `hotkeyPressBehavior` | §1.11 |
| 7 | Providers: streaming `start/feed/stop/cancel` protocol (out of scope here; see providers spec) | — |
| 8 | Session: refresh coalescing | §8.4 |
| 9 | Pipeline: per-provider watchdog + `SafeCompletion` gate | §2.5, §4 |
| 10 | Injector: Esc cancel + 8-char focus check in Natural Mode | §1.5 |
| 11 | Overlay: `Inserting` state at all four inject sites | §1.4 |
| 12 | Tray: Undo last dictation, Recent dictations, Provider, Setup & Permissions, version footer, crash reports, attention badge | §1.6, §5, §3 |
| 13 | Onboarding wizard + `PermissionsManager` parity (`IOHIDCheckAccess`, `isRequired`, rationales, pollers) | §3 |
| 14 | AudioRecorder: RMS level callback with the −60…0 dBFS mapping | §1.7 |
| 15 | Telemetry ring buffer (in-memory, 10 entries, never transmitted) | §5 |
| 16 | Lifecycle: startup/shutdown ordering, log rotation, log drain before close | §6 |
| 17 | AX context: four-attribute × two-level ladder with 50 ms timeouts, documented as wontfix | §1.2 |
| 18 | Recovery: `< 90 s` silent auto-retry; 24 h opportunistic sweep | §2 |
| 19 | Clipboard `changeCount` guard; `HistoryStore::prune`; `VOCABULARY_LIMIT` 50 → 500 | §8 |
| 20 | Settings `.bak` snapshot + 100 ms debounced save | §7 |
