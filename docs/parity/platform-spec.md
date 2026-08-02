# Wispr Lightning — Platform Integration Layer Behavioral Spec

Source: `/Users/mike/Code/Default/wispr-lightning/Sources/WisprLightning/Services/` (+ callers in `App/AppDelegate.swift`, constants in `Constants.swift`, defaults in `Models/Settings.swift`).

---

## 0. Global constants (`Constants.swift`)

| Name | Value |
|---|---|
| `apiURL` | `"https://api.wisprflow.ai"` |
| `sampleRate` | `16000` (Hz) |
| `channels` | `1` |
| `chunkDurationMs` | `40` |
| `chunkSamples` | `16000 * 40 / 1000` = **640** samples → **1280 bytes** per packet (Int16) |
| `clientVersion` | `"1.4.549"` |
| `maxRecordingSeconds` | `600` |
| `warningSeconds` | `540` |
| `finalWarningSeconds` | `570` |

Settings file (JSON, pretty-printed): `~/Library/Application Support/WisprLightning/settings.json`. On save it posts notification `"WisprLightningSettingsChanged"`. Other notification names: `"WisprSessionChanged"`, `"WisprPreviewSoundPack"`, `"WisprAudioDevicesChanged"`.

Relevant defaults: `hotkeyKeyCode=59`, `hotkeyLabel="Left Control"`, `hotkeyKeyCodes=[59]`, `hotkeyLabels=["Left Control"]`, `micDeviceUID=nil`, `micDeviceName=nil`, `keepMicrophoneActive=false`, `enableSounds=true`, `muteMusic=false`, `useScreenContext=false`, `useAccessibilityContext=true`, `polishEnabled=false`, `polishHotkeyKeyCodes=[62]` (Right Control), `polishHotkeyLabels=["Right Control"]`, `autoPolish=false`, `selectedSoundPack=nil`, `hotkeyPaused=false`, `naturalModeEnabled=false`, `naturalModeSpeed="normal"`.

---

## 1. `AudioRecorder.swift` (387 lines)

### Audio format — exact
- **Capture graph**: `AVAudioEngine` (macOS-only, AVFoundation) → tap on `inputNode` bus 0.
- **Tap format**: the *hardware* format `inputNode.inputFormat(forBus: 0)` (whatever the device natively provides — arbitrary sample rate, float32, N channels).
- **Tap buffer size request**: `AVAudioFrameCount(640)` (= `chunkSamples`). This is a *request*; CoreAudio may deliver larger buffers, which the chunker handles.
- **Target format**: `AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16000, channels: 1, interleaved: true)` → **16 kHz, mono, signed 16-bit little-endian, interleaved**.
- **Conversion**: `AVAudioConverter(from: hwFormat, to: targetFormat)` — cached in `cachedConverter` and reused only if BOTH `inputFormat == hwFormat` and `outputFormat == targetFormat`; otherwise recreated. Performs both resampling and channel downmix and int conversion in one step.
- **Per-buffer conversion** (`processBuffer`): `ratio = 16000 / hwSampleRate`; output capacity `= frameLength * ratio` (truncated to `AVAudioFrameCount`). Uses the pull-block form of `convert(to:error:withInputFrom:)` supplying the input buffer exactly once then returning `.noDataNow`. If `error != nil` or `frameLength == 0`, the buffer is dropped silently.
- **Chunking**: from the converted Int16 buffer, emit `Data` slices of exactly `640` samples (`1280` bytes) while `offset + 640 <= totalSamples`. **Remainder samples at the tail of each converted buffer are DISCARDED** (no carry-over ring buffer). This is a real behavior to replicate (or knowingly improve) — with typical buffer sizes it drops <40 ms per callback.
- Packets accumulate in `packets: [Data]`, guarded by `packetsLock` (`NSLock`). Tap callback early-returns unless `isRecording`.

### Device selection by UID
- `settings.micDeviceUID` (String?) selects the input. `nil` → system default, and `selectConfiguredDevice()` returns `true` immediately (no device manipulation).
- **Mechanism (macOS-only, CoreAudio HAL)**: the app changes the **system-wide default input device** (`kAudioHardwarePropertyDefaultInputDevice` on `kAudioObjectSystemObject`, global scope, main element) via `AudioObjectSetPropertyData`. It does NOT set a per-engine device. Side effect: selecting a mic in this app changes the mic for the whole machine.
- Fast path: `cachedDeviceUID`/`cachedDeviceID` (guarded by `cacheLock`). If the requested UID matches the cache, `setInputDeviceDirect(cachedID)` is used; on failure the cache is invalidated and the slow path runs.
- Slow path `setInputDevice(uid:)`: enumerate `kAudioHardwarePropertyDevices`, for each device read `kAudioDevicePropertyDeviceUID` (CFString), string-compare with the requested UID; on match call `setInputDeviceDirect` and populate the cache.
- **Device missing** → `setInputDevice` returns `false` → log `"Wispr Lightning: Requested mic '<name|uid>' not available, using system default"` → `selectConfiguredDevice()` returns `false` → `start()` returns `.startedWithFallback` (recording proceeds on the current system default). AppDelegate logs `"Recording started with fallback mic (requested device unavailable)"`; no user-visible UI.
- `listInputDevices() -> [(uid, name)]`: enumerate all devices; keep only those where `kAudioDevicePropertyStreamConfiguration` with **input scope** reports `mNumberBuffers > 0 && mBuffers.mNumberChannels > 0`; read `kAudioDevicePropertyDeviceUID` and `kAudioDevicePropertyDeviceNameCFString`.

### Device-change / hot-plug handling
- Two **distinct** `AudioObjectPropertyListenerBlock` registrations on `kAudioObjectSystemObject` (comment explicitly notes blocks match by identity, so a shared block would register once and leak):
  1. `kAudioHardwarePropertyDevices` (device list changed)
  2. `kAudioHardwarePropertyDefaultInputDevice` (default input changed)
  Each: invalidate the UID/ID cache, then post `.audioDevicesChanged` on the main queue.
- Also observes `AVAudioEngineConfigurationChange` for this engine → logs `"Wispr Lightning: AVAudioEngine configuration changed"`, invalidates cache, posts `.audioDevicesChanged`.
- AppDelegate reaction to `.audioDevicesChanged`: refresh status-bar menu; **if recording**, check whether the configured UID still exists and log `"Target mic '<name>' disconnected during recording"` (recording is NOT stopped, packets simply stop arriving); **if not recording**, call `rearmMicrophone()`.
- `rearmMicrophone()` (also fired on any settings change): debounce `Timer` of **0.15 s**, then `deactivate()` and, if `keepMicrophoneActive`, `prewarm()` again.

### Pre-warm (`keepMicrophoneActive`)
- Setting default `false`. UI copy: “Eliminates startup delay — recommended when using iPhone as microphone”.
- On launch, if enabled → `prewarm()`.
- `prewarm()`: no-op if already prewarmed or recording. Selects configured device, installs tap, starts engine, sets `isPrewarmed = true`, logs `"Wispr Lightning: Microphone pre-warmed (input: %@)"`. On throw: logs failure and removes the tap. **The tap is installed but discards everything because `isRecording` is false.** The mic-in-use indicator stays lit while pre-warmed.
- `deactivate()`: only when prewarmed and not recording → remove tap, stop engine, `isPrewarmed = false`, log `"Wispr Lightning: Microphone deactivated"`.
- **`stop()` always sets `isPrewarmed = true` and leaves the engine running**, regardless of the `keepMicrophoneActive` setting. Comment: “Keep engine running — prevents CoreAudio reconfiguration that causes Bluetooth audio dropout.” So after the first dictation the mic stays hot until a settings change / device change triggers `rearmMicrophone()`.

### start / stop lifecycle
- `start() -> StartResult { started, startedWithFallback, failed(String) }`:
  1. `packets = []`, `isRecording = true`.
  2. If `isPrewarmed` and `audioEngine.isRunning` → return `.started` (log “Recording started (prewarmed mic)”). If prewarmed but engine died → remove tap, `isPrewarmed = false`, fall through.
  3. `fellBack = !selectConfiguredDevice()`.
  4. `setupAndStartEngine()`; on success log `"Audio engine started (input: %@, rate: %.0f Hz)"` and return `.startedWithFallback` if `fellBack` else `.started`.
  5. On throw: log, remove tap, `isRecording = false`, return `.failed(localizedDescription)` → AppDelegate shows overlay error **"Mic unavailable"**, resets state to idle, resumes music.
- `stop() -> [Data]`: `isRecording = false`, snapshot packets under lock, log `"Recording stopped — %d packets (%.1fs)"` using `count * 40 / 1000`, set `isPrewarmed = true`, return packets.
- `cleanup()`: remove tap, stop engine, drop converter.

### Max duration / warnings (in AppDelegate, driven by AudioRecorder data)
- A repeating `Timer` at **1.0 s** while recording. `elapsed = Int(now - recordingStartTime)`.
  - `elapsed >= 600` → log `"Max recording duration reached (600s), auto-stopping"` → `stopRecordingSession()`.
  - else `elapsed >= 570` → `recordingOverlay.showFinalWarning()`.
  - else `elapsed >= 540` → `recordingOverlay.showWarning()`.
  - always `recordingOverlay.updateElapsed(elapsed)`.
- **Minimum length gate**: after stop, if `packets.count < 5` (< 200 ms) the recording is discarded. Sub-case: `packets.count == 0 && elapsed > 1.0 s` → overlay error **"Mic not responding"** + log “likely mic disconnected”; otherwise log `"Too short (N packets), ignoring"` and hide overlay. Music is resumed in both cases.

### Audio level / RMS — IMPORTANT CORRECTION
- **`AudioRecorder` computes and publishes NO audio level at all.** There is no metering, no `@Published` level, and the recording overlay receives only elapsed seconds (`updateElapsed`) and state changes — no waveform data. Grep for `level|amplitude|waveform|rms` across the app finds no live meter.
- RMS exists only **post-hoc at upload time** in `TranscriptionClient.prepareAudio(packets:)`: per 640-sample packet, `rms = sqrt(Σ s² / 640)` over Int16 samples, then `volume = round(rms / 32768 * 10000) / 10000` (i.e. normalized 0…1 rounded to 4 decimals). These are sent as the `volumes` array alongside `packet_duration = 0.04`, `audio_encoding = "wav"`, `byte_encoding = "ascii85"`.

### Permissions
- **No explicit permission API is called for the microphone.** No `AVCaptureDevice.requestAccess`. The TCC prompt is raised implicitly by `audioEngine.start()`; the string comes from `Resources/Info.plist` key `NSMicrophoneUsageDescription` = “Wispr Lightning needs microphone access to record your voice for dictation.” If denied, `start()` throws → `.failed` → overlay **"Mic unavailable"**.

---

## 2. `HotkeyListener.swift` (230 lines)

### API used — explicitly NOT CGEventTap
- Uses **`NSEvent.addGlobalMonitorForEvents` + `addLocalMonitorForEvents`** (AppKit). Three monitors installed in `installMonitors()`:
  1. global `.flagsChanged`, path label `"global-flags"`
  2. local `.flagsChanged`, path label `"local-flags"` (returns the event unmodified)
  3. global `[.keyDown, .keyUp]`, path label `"global-key"`
- Source comment states CGEventTap is *intentionally* not installed: an event tap sits below OS dispatch and would fire even when **Universal Control** routes the keypress to another Mac. NSEvent global monitors fire only for events dispatched on this Mac.

### Keycode table (`keycodeLabels`, exact)
`59 Left Control`, `62 Right Control`, `58 Left Option`, `61 Right Option`, `55 Left Command`, `54 Right Command`, `56 Left Shift`, `60 Right Shift`, `63 Fn`, `36 Return`, `49 Space`, `53 Escape`, `48 Tab`. Modifier keycode set used for branching: `{59,62,58,61,55,54,56,60,63}`.

### Hotkey set resolution
- `rebuildHotkeySet()` (on init-time `.settingsChanged` observer, on `start()`, on `rebind()`): `_hotkeySet = settings.hotkeyKeyCodes.isEmpty ? {settings.hotkeyKeyCode} : Set(hotkeyKeyCodes)`; `_polishKeyCodes = Set(settings.polishHotkeyKeyCodes)`.
- **There is NO chord/combo matching.** `hotkeyKeyCodes` is a set of *independent alternative triggers* (“press A **or** B”), not a simultaneous combination. The startup log is `"Hotkey listener active (press <labels joined by \" or \">to dictate)"`.
- `rebind(keyCode:)`: remove monitors → set `hotkeyKeyCode`, `hotkeyLabel = keycodeLabels[k] ?? "Key <k>"`, `hotkeyKeyCodes = [k]`, `hotkeyLabels = [label]` → save → rebuild → reinstall monitors.

### Modifier-only key press vs release detection
Modifier keys never emit keyDown/keyUp; they emit `flagsChanged`. Press vs release is inferred from the *flags* in that event via `isModifierDown(keycode:flags:)`:
- `59, 62` → `flags.contains(.control)`
- `58, 61` → `.option`
- `55, 54` → `.command`
- `56, 60` → `.shift`
- `63` → `.function`
- default → `false`

**Consequence to replicate:** left and right variants of the same modifier share one flag bit. Holding Left Control and then pressing Right Control keeps the flag set, so a release of one while the other is held is not seen as a release.

### Two guard conditions applied to every trigger
1. `isCursorOnLocalDisplay()` — `NSScreen.screens.contains { $0.frame.contains(NSEvent.mouseLocation) }`. If the pointer has moved to another Mac/display not owned by this machine (Universal Control), the hotkey is ignored.
2. `isLocalHIDEvent(event)` — `event.cgEvent?.getIntegerValueField(.eventSourceUnixProcessID) == 0`, i.e. the event was posted by the kernel from real HID hardware, not synthesized by another process. If `event.cgEvent` is nil it defaults to `true`. This rejects Universal Control's re-posted flagsChanged from the other Mac, and rejects synthetic events from other apps.

### Dictation state machine inside the listener (`keyDown`, `activeKeyCode`)
- **flagsChanged path** (modifier hotkeys), for keycode ∈ hotkeySet:
  - `pressed && !keyDown && onScreen && localHID && !isPaused` → `keyDown = true`, `activeKeyCode = keycode`, call `onPress()`.
  - `!pressed && keyDown && activeKeyCode == keycode` → `keyDown = false`, `activeKeyCode = nil`, call `onRelease()`.
  - Note the asymmetry: **release is NOT gated** on `onScreen`, `localHID`, or `isPaused` — so a recording started locally always gets its release.
- **keyDown/keyUp path** (non-modifier hotkeys): identical logic, additionally `guard !isModifierKeycode(event.keyCode)`.
- **No debounce, no repeat-suppression** other than the `keyDown` latch (which naturally swallows auto-repeat keyDowns).
- Every evaluation emits a verbose log line, e.g. `Hotkey[global-flags] keycode=59 pressed=true onScreen=true localHID=true paused=false keyDown=false`.
- `resetState()` / `removeMonitors()` clear `keyDown` and `activeKeyCode`.

### Tap-vs-hold, lock mode, trailing buffer (in `AppDelegate`, thresholds exact)
Constants: `lockDebounceInterval = 0.5 s`, `trailingBufferInterval = 0.5 s`. State enum: `.idle | .listening | .recording` (`.recording` = hands-free “locked”).

`onHotkeyPress()`:
- `.idle` → `state = .listening`, `lastPressTime = now`, `startRecordingSession()`.
- `.listening` → cancel `tapDelayTimer`; `elapsed = now - lastPressTime`; if `elapsed < 0.5` → `state = .recording`, `lastPressTime = now`, log `"Recording locked — hands-free mode"`, `overlay.showLocked()`. Else → `stopRecordingSession()`.
- `.recording` → `stopRecordingSession()`.

`onHotkeyRelease()`:
- Ignored unless `state == .listening` (in locked mode release does nothing).
- `heldDuration = now - lastPressTime` (defaults to `1.0` if nil).
- If `heldDuration >= 0.5` (a real push-to-talk hold): schedule a one-shot timer at **0.5 s** (trailing buffer, to capture the tail of speech) which stops the session if still `.listening`.
- Else (quick tap): schedule a one-shot timer at **`0.5 - heldDuration`** — i.e. the tap-lock window ends exactly 0.5 s after the *first press* — which stops the session if still `.listening`.

**So double-tap detection = “second press within 0.5 s of the first press ⇒ hands-free lock”.** A single quick tap starts recording and auto-stops at T+0.5 s from press.

### Pause
- `isPaused` reads `settings.hotkeyPaused` (persisted in settings.json, survives relaunch).
- `setPaused(_:)` — no-op if unchanged; writes + saves settings, logs `"Hotkey paused"` / `"Hotkey resumed"`, and clears `keyDown`/`activeKeyCode` so a physically-held key isn't stuck across the toggle.
- While paused, **press** handlers early-return (both dictation and polish). **Release** handlers still run (see asymmetry above). Toggled from the status-bar menu (`onTogglePause`).

### Polish hotkey — how it differs
- Separate keycode set `settings.polishHotkeyKeyCodes` (default `[62]`, Right Control).
- Handled BEFORE the dictation check, and only when the keycode is **not** also in the dictation hotkey set (`_polishKeyCodes.contains(k) && !hotkeySet.contains(k)`).
- **Press-only / edge-triggered**: fires on modifier-down (or on `.keyDown` for regular keys). No release handling, no hold semantics, no lock mode.
- Rate limit: `triggerPolish()` ignores triggers within **0.5 s** of the previous accepted trigger (`lastPolishTriggerTime`).
- Requires `settings.polishEnabled` and a non-nil `onPolishPress` handler; the handler is dispatched `DispatchQueue.main.async`.
- Same `onScreen && localHID && !isPaused` gating.

### Permissions (Accessibility / Input Monitoring)
- **The listener performs no permission check whatsoever** and has no failure path. If Input Monitoring is denied, `NSEvent.addGlobalMonitorForEvents` returns a monitor object but no events arrive — the app looks alive and simply never triggers. There is **no user-facing diagnostic for this case**.
- The only permission call in the app is in `AppDelegate.applicationDidFinishLaunching`: `AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt: true])` (ApplicationServices) — this prompts for **Accessibility** (needed for text injection) and logs either `"Accessibility: trusted"` or a message pointing at System Settings > Privacy & Security > Accessibility.

---

## 3. `TextInjector.swift` (328 lines)

### Strategy selection — there are exactly two, chosen by a setting (not a fallback chain)
`inject(text:completion:)`:
1. `guard !text.isEmpty` else `completion(false)`.
2. Log `"TextInjector.inject called with N chars"`.
3. Dispatch onto serial queue `"com.wisprlightning.textinjection"`.
4. **`Thread.sleep(0.01)`** — 10 ms, “to ensure hotkey release is fully processed”.
5. If `settings.naturalModeEnabled == true` → `typeAsKeystrokes`; else → `pasteViaClipboard`.

There is **no AX `AXUIElementSetAttributeValue` write path**. AX is used only for *reading* (context, selection, paste verification).

### Strategy A — clipboard + synthetic Cmd+V (`pasteViaClipboard`, the default)
Exact sequence:
1. `saveClipboard()` — on the main thread, snapshot **every** `NSPasteboardItem` and **every** type on each item as `[(type, Data)]` pairs; items with no readable data are skipped. Returns `[[(type, Data)]]`.
2. Main-thread sync: `NSPasteboard.general.clearContents()`, `setString(text, forType: .string)`.
3. Log `"Clipboard set, simulating Cmd+V"`.
4. `CGEventSource(stateID: .hidSystemState)`; `CGEvent(keyboardEventSource:virtualKey: 9, keyDown: true/false)` — **virtual key 9 = `V`**; `flags = .maskCommand` on both; posted to `.cghidEventTap`. If event creation fails → log `"Failed to create Cmd+V CGEvent — check Accessibility permissions"`, `completion(false)`, and **the clipboard is never restored** (bug worth not porting).
5. Log `"Cmd+V posted"`.
6. `Thread.sleep(0.05)` — **50 ms** wait for the target app to consume the paste.
7. `verifyPaste(expected:)`.
8. Schedule clipboard restore on main queue at **+0.25 s** (`asyncAfter`) → `restoreClipboard(savedItems)`; if non-empty, log `"Clipboard restored (%d items)"`. Restore = `clearContents()` then re-create one `NSPasteboardItem` per saved item, `setData(forType:)` for each type, `writeObjects([item])` **one item at a time** (note: each `writeObjects` call appends; ordering preserved).
9. `completion(pasteOK)`; on failure additionally logs `"Paste verification failed — clipboard still restored"`.

`verifyPaste(expected:)` — Accessibility read-back:
- `AXUIElementCreateSystemWide()` → `kAXFocusedUIElementAttribute` → `kAXValueAttribute`.
- If there is no focused element → log “no focused element — assuming success”, return **true**.
- If the value attribute is unreadable/not a String → log “could not read value attribute — assuming success”, return **true**.
- Otherwise return `focusedValue.contains(String(expected.prefix(20)))` — the **first 20 characters** must appear in the field's text.

### Strategy B — Natural Mode (character-by-character typing)
`typeAsKeystrokes`:
- Speed presets → **chars per second**: `"slow" → 2.5` (≈30 WPM), `"normal"/default → 4.0` (≈50 WPM), `"expert" → 6.5` (≈80 WPM). `baseDelay = 1.0 / cps` → slow 400 ms, normal 250 ms, expert ≈153.8 ms per character.
- **Per-character inter-key delay = `baseDelay * random(0.6...1.4)`** (±40 % uniform jitter). Ranges: slow 240–560 ms, normal 150–350 ms, expert 92.3–215.4 ms.
- **Key hold time (down→up) = `random(0.030...0.080)` s** — 30–80 ms, applied to every key including the unicode fallback. Comment: “ensures fast-key detectors register a press, not a glitch.” Effective throughput is therefore `baseDelay*jitter + hold`.
- Event source: **`CGEventSource(stateID: .privateState)`** — deliberately isolated from hardware modifier state (Caps Lock, residual Shift from the hotkey) which would otherwise corrupt characters (`,`→`<`, `'`→`"`). If source creation fails → log `"Natural Mode: failed to create CGEventSource — falling back to paste"` and **fall back to `pasteViaClipboard`**. This is the only inter-strategy fallback.
- Before typing: `DispatchQueue.main.sync { ensureLayoutMap() }` (TIS APIs assert main thread), then log `"Natural Mode typing N chars at C cps (layout map: M entries)"`.
- Per character (`postCharacter`):
  - `\n` → real Return, **virtual key 36**, flags `[]`.
  - `\t` → real Tab, **virtual key 48**, flags `[]`.
  - character present in `layoutMap` → `postKey(virtualKey:flags:)` with the mapped code+flags. Flags are **always pinned** (`down.flags = flags; up.flags = flags`) even when empty, so ambient modifiers cannot ride along.
  - otherwise → `postUnicodeFallback`: `CGEvent(virtualKey: 0)` down/up with `flags = []` and `keyboardSetUnicodeString(...)` set from the character's UTF-16 units on both events.
  - All events posted to `.cghidEventTap`.
- `completion(true)` unconditionally at the end (no verification in Natural Mode).

`ensureLayoutMap()` — reverse keyboard-layout map (Carbon/HIToolbox, macOS-only):
- `TISCopyCurrentKeyboardLayoutInputSource()`; identity cached by `kTISPropertyInputSourceID`; rebuilt when the input source ID changes or the map is empty.
- `kTISPropertyUnicodeKeyLayoutData` → `UCKeyboardLayout`; `LMGetKbdType()` for keyboard type.
- Iterates **virtual keys 0..<128** × 4 modifier combos: `(0, [])`, `(shiftKey>>8, .maskShift)`, `(optionKey>>8, .maskAlternate)`, `((shiftKey|optionKey)>>8, [.maskShift, .maskAlternate])`. **Command is deliberately excluded** (it suppresses character generation).
- `UCKeyTranslate` with `kUCKeyActionDown` and `kUCKeyTranslateNoDeadKeysBit`, 4-unit output buffer. Accepts a result only if `err == noErr`, `actualLen > 0`, the produced string is exactly **one Character**, and its first scalar is **≥ 0x20** (control characters skipped; `\n`/`\t` handled explicitly). **First combo wins** (`if newMap[ch] == nil`), so the unshifted mapping is preferred.
- If layout data is unavailable → log `"Natural Mode: keyboard layout data unavailable"` and return with the old/empty map (everything then goes through the unicode fallback).

### Static AX helpers (used by callers)
- `readSelectedText() -> String?` — system-wide AX element → `kAXFocusedUIElementAttribute` → `kAXSelectedTextAttribute`; nil if absent or empty. (Present but *unused* by the polish flow, which prefers synthetic Cmd+C — see §7.)
- `readFocusedElementText() -> [String]` — same but `kAXValueAttribute`; returns `[text]` or `[]`. Used as the “accessibility context” fed to the transcription API when `settings.useAccessibilityContext` (default **true**), captured on a background queue at recording start.
- `saveClipboard()` / `restoreClipboard(_:)` — as described; `saveClipboard` must be called off-main (it does `DispatchQueue.main.sync` internally), `restoreClipboard` must be called on main.

### Per-app special-casing
**None.** There are zero hardcoded bundle IDs in `TextInjector.swift`. The only bundle-ID lists in this layer live in `AppInfoDetector` and `MusicController`.

---

## 4. `ScreenCaptureContext.swift` (68 lines)

Single static `captureOCRContext() -> [String]`. Called only when `settings.useScreenContext` (default **false**), on a dedicated `ocrQueue`, **at recording start** (runs in parallel with recording), and drained at stop.

Pipeline (all steps macOS-only):
1. `CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)`. Failure → log `"ScreenCaptureContext: Failed to get window list"`, return `[]`.
2. `NSWorkspace.shared.frontmostApplication` → `processIdentifier`. Nil → log `"No frontmost application"`, return `[]`.
3. Pick the **first** window whose `kCGWindowOwnerPID == frontPID` **and** `kCGWindowLayer == 0` (normal layer, excludes panels/menus); take `kCGWindowNumber`. None → log `"No window found for frontmost app"`, return `[]`.
4. `CGWindowListCreateImage(.null, .optionIncludingWindow, windowID, [.boundsIgnoreFraming])` — captures **only that one window**, full window bounds, no frame/shadow. Nil → log `"Screen capture returned nil — likely missing Screen Recording permission"`, return `[]`.
5. Vision OCR: `VNImageRequestHandler(cgImage:options:[:])` + `VNRecognizeTextRequest` with `recognitionLevel = .fast` and `usesLanguageCorrection = false`. No language list, no ROI, no minimum text height, **no timeout of any kind** (synchronous `perform`). Throw → log `"Vision OCR failed: <desc>"`, return `[]`.
6. Results: for each observation take `topCandidates(1).first?.string`; append; **break once `lines.count >= 50`**. No confidence filtering, no dedup, no length filter, no sorting (Vision's natural observation order).

**Permission handling**: none — no `CGPreflightScreenCaptureAccess`/`CGRequestScreenCaptureAccess`. Denial manifests as a nil image and an empty result with only a log line. Uses the legacy `CGWindowListCreateImage` (deprecated in macOS 14 in favor of ScreenCaptureKit) — ScreenCaptureKit is **not** used.

---

## 5. `AppInfoDetector.swift` (40 lines)

`getFrontmostAppInfo() -> [String: String]` — always exactly **four keys**: `"name"`, `"bundle_id"`, `"type"`, `"url"`.

- Source: `NSWorkspace.shared.frontmostApplication` (AppKit). Nil → `["name":"", "bundle_id":"", "type":"other", "url":""]`.
- `name = app.localizedName ?? ""`; `bundle_id = app.bundleIdentifier ?? ""`.
- `type` classification, checked in this order, exact bundle-ID sets:
  - `"messaging"`: `com.slack.Slack`, `com.tinyspeck.slackmacgap`, `net.whatsapp.WhatsApp`, `com.tdesktop.Telegram`, `org.whispersystems.signal-desktop`, `com.discordapp.Discord`
  - `"email"`: `com.apple.mail`, `com.microsoft.Outlook`, `com.google.Gmail`
  - `"ai"`: `com.openai.chat`, `com.anthropic.claudefordesktop`, `com.todesktop.230313mzl4w4u92` (Cursor), `com.microsoft.VSCode`
  - else `"other"`
- **`url` is ALWAYS the empty string.** There is no browser-URL detection anywhere — no AppleScript to Safari/Chrome, no AX URL read. Do not port a URL feature; port the empty field.
- Consumers: sent to the transcription API as app context; `type == "email"` additionally triggers the email signature suffix (`"\n\n— Spoken with Wispr Lightning"` when `emailSignatureOption == "spoken_with_lightning"`, else `"\n\n— Written with Wispr Lightning"`) when `settings.emailAutoSignature`.
- Captured at **recording start** (`startRecordingSession`), not at injection time.

---

## 6. `MusicController.swift` (76 lines)

Gated entirely by `settings.muteMusic` (default **false**); both `pauseMusic()` and `resumeMusic()` early-return when off.

### Detection
- `isAppRunning(bundleId)` = `NSWorkspace.shared.runningApplications.contains { $0.bundleIdentifier == bundleId }`. Only two apps: **`com.apple.Music`** and **`com.spotify.client`**.
- Whether music is *playing* is determined inside AppleScript by `player state is playing`.

### Exact AppleScript sources (verbatim, `\n` = real newlines)
```
tell application "Music" to if player state is playing then
pause
return "paused"
end if
```
```
tell application "Spotify" to if player state is playing then
pause
return "paused"
end if
```
Resume:
```
tell application "Music" to play
```
```
tell application "Spotify" to play
```
Run via `NSAppleScript(source:).executeAndReturnError(&error)`, reading `result.stringValue`. Errors are swallowed (no logging).

### Pause/resume logic
- `pauseMusic()`: for each running app, `DispatchGroup.enter()` + `DispatchQueue.global(qos: .userInitiated).async`, run the script, set `musicWasPlaying` / `spotifyWasPlaying = (result == "paused")` under an `NSLock`, `leave()`. Then **`group.wait()` — blocks the calling thread until both finish.** Callers therefore invoke it from a background queue (`startRecordingSession` dispatches it to `.userInitiated`).
- `resumeMusic()`: under lock, read+clear both flags, then issue the `play` scripts for whichever were true. Resume runs on whatever thread the caller uses (AppDelegate has `resumeMusicInBackground()`).

### Race conditions present in the original (port deliberately or fix knowingly)
1. `pauseMusic()` is fired-and-forgotten at recording start on a background queue; a very short recording can call `resumeMusic()` **before** the pause script has stored its flag → music stays paused forever (or gets paused after the resume).
2. The flags are cleared on read, so a second `resumeMusic()` is a no-op — but a failed/slow pause is never retried.
3. AppleScript round-trips take tens to hundreds of ms; the comment in AppDelegate says “AppleScript calls are slow”, which is why it is off the main thread.

---

## 7. `SoundManager.swift` (108 lines) + `Resources/Sounds`

### Files on disk
```
Resources/Sounds/
  default/  dictation-start.wav (31.1 KB)  dictation-stop.wav (37.8 KB)  paste.wav (58.1 KB)
  v1/       achievement.wav (216.1 KB)  dictation-start.wav (134.0 KB)  dictation-stop.wav (134.0 KB)
            Notification.wav (134.0 KB)  paste.wav (58.1 KB)  popo-lock.wav (267.9 KB)
  v2/       achievement.wav (216.1 KB)  dictation-start.wav (134.0 KB)  dictation-stop.wav (134.0 KB)
            Notification.wav (267.9 KB)  paste.wav (58.1 KB)  popo-lock.wav (267.9 KB)
  v3/       achievement.wav (216.1 KB)  dictation-start.wav (134.0 KB)  dictation-stop.wav (134.0 KB)
            Notification.wav (267.9 KB)  paste.wav (58.1 KB)  popo-lock.wav (267.9 KB)
```
**Only three names are ever loaded**: `dictation-start.wav`, `dictation-stop.wav`, `paste.wav`. `achievement.wav`, `Notification.wav`, `popo-lock.wav` are shipped but **never referenced in code** (dead assets).

### Pack resolution
- `packName = settings.selectedSoundPack ?? "default"`.
- `soundURL(name:pack:)`: `Bundle.main.url(forResource: name, withExtension: "wav", subdirectory: "Sounds/<pack>")`; if that fails **and** pack != `"default"`, retry in `"Sounds/default"`; else nil.
- `availablePacks()`: list subdirectories of the bundle's `Sounds` folder, `lastPathComponent`, **sorted alphabetically**; `["default"]` if the folder is missing or empty. (Current result: `["default", "v1", "v2", "v3"]`.)
- Three `AVAudioPlayer`s created with `try?` (silently nil on failure) and `prepareToPlay()` called on each.
- Reloads the pack on every `.settingsChanged` notification. Also observes `.previewSoundPack` (`"WisprPreviewSoundPack"`) → plays the **start** sound as a preview.

### Triggers and behavior
| Method | Trigger | Behavior |
|---|---|---|
| `playStart()` | `startRecordingSession()` (immediately **before** `audioRecorder.start()`); also `onPolishHotkeyPress()` before the Cmd+C; also `.previewSoundPack` | `player.currentTime = 0; play()`. If the player is nil → **`NSSound(named: "Tink")`** system sound |
| `playStop()` | `stopRecordingSession()` (immediately after `audioRecorder.stop()`); and after a successful polish injection (inside the +0.3 s clipboard-restore block) | `currentTime = 0; play()`. Nil player → **`NSSound(named: "Pop")`** |
| `playPaste()` | **Never called anywhere in the codebase** — dead code; `paste.wav` is loaded but never played. No system-sound fallback | `currentTime = 0; play()` |

- All three are gated by `settings.enableSounds` (default **true**).
- **No volume is ever set** — `AVAudioPlayer.volume` stays at its default `1.0`; there is no volume setting in `AppSettings`. Playback is fire-and-forget on the default output device, overlapping playback allowed (restarting a still-playing sound just seeks to 0).

---

## 8. End-to-end sequence (for the port's state machine)

```
PRESS (state .idle)
  -> state=.listening, lastPressTime=now
  -> AppInfoDetector.getFrontmostAppInfo()          (frontmost app snapshot)
  -> soundManager.playStart()
  -> audioRecorder.start()                          (.failed => overlay "Mic unavailable", state=.idle)
  -> recordingStartTime=now
  -> bg: musicController.pauseMusic()               (AppleScript, blocking on its own thread)
  -> transcriptionClient.prewarmConnection()        (TCP+TLS)
  -> if useAccessibilityContext: axQueue  -> TextInjector.readFocusedElementText()
  -> if useScreenContext:        ocrQueue -> ScreenCaptureContext.captureOCRContext()
  -> statusBar.setRecording(true); overlay.show()
  -> 1.0 s repeating timer (elapsed / 540 warn / 570 final warn / 600 auto-stop)

SECOND PRESS within 0.5 s  -> state=.recording (locked, hands-free), overlay.showLocked()
RELEASE while .listening:
   held >= 0.5 s -> stop after 0.5 s trailing buffer
   held <  0.5 s -> stop at (first press + 0.5 s)
PRESS while .recording     -> stop

STOP
  -> state=.idle, timers invalidated
  -> packets = audioRecorder.stop()   (engine stays running, isPrewarmed=true)
  -> soundManager.playStop(); statusBar.setRecording(false)
  -> packets.count < 5 ?  discard ("Mic not responding" if 0 packets and >1.0 s elapsed) + resumeMusic
  -> overlay.showProcessing(); save raw PCM to disk (crash recovery)
  -> processing timeout = max(30, 30 + duration*0.5) s -> retryable-error UI
  -> transcribe (auto-retry on retryable errors, 1.5 s backoff)
  -> on success: append email signature if type=="email" && emailAutoSignature
                 textInjector.inject(text)   [skipped when autoPolish && polishEnabled]
                 history write + auto-learn on utility queue

POLISH PRESS (edge, >=0.5 s since last, polishEnabled, non-empty instructions)
  -> playStart(); overlay.show()+showProcessing()
  -> bg: saveClipboard(); synthetic Cmd+C (virtual key 8, .maskCommand, .cghidEventTap)
  -> sleep 0.15 s; read NSPasteboard string
  -> empty -> restoreClipboard + overlay "Select text to polish"
  -> polishService.polish(...) -> textInjector.inject(polished)
     -> +0.3 s: restoreClipboard(original), playStop(), overlay.hide()

SYSTEM SLEEP (NSWorkspace.willSleepNotification) while recording
  -> state=.idle, all timers killed, hotkeyListener.resetState(),
     audioRecorder.stop() with packets DISCARDED, prewarm connection cancelled,
     statusBar off, overlay.hide(), resumeMusic
```

---

## 9. macOS-only APIs and what they DO (behavior, not name)

| API / framework | Used for (the behavior to reimplement) | Windows equivalent direction |
|---|---|---|
| `AVAudioEngine` + input node tap (AVFoundation) | Continuous mic capture in the hardware's native format, with the engine kept running between recordings to avoid Bluetooth re-negotiation | WASAPI shared-mode capture client, keep the stream open |
| `AVAudioConverter` / `AVAudioFormat` | Resample+downmix+quantize arbitrary hardware format → 16 kHz mono S16LE interleaved | Manual resampler (e.g. `rubato`/`dasp`) + channel mixdown, or WASAPI's format conversion |
| CoreAudio HAL `AudioObjectGetPropertyData`/`SetPropertyData` on `kAudioObjectSystemObject` | Enumerate input devices with UID+name; **change the system default input device** to the user's chosen mic; detect input-capable devices via stream configuration | `IMMDeviceEnumerator` for enumeration (`IMMDevice` ID string ≈ UID, `PKEY_Device_FriendlyName` ≈ name). Windows can select a capture endpoint **per-stream** — do that instead of changing the system default |
| `AudioObjectAddPropertyListenerBlock` (device list / default input) | Hot-plug and default-device-change notifications → invalidate cache, re-arm mic, refresh menu | `IMMNotificationClient` (`OnDeviceAdded`/`OnDeviceRemoved`/`OnDefaultDeviceChanged`) |
| `AVAudioEngineConfigurationChange` notification | Detect engine reconfiguration (device switch mid-session) | WASAPI `AUDCLNT_E_DEVICE_INVALIDATED` on the render/capture client |
| `NSMicrophoneUsageDescription` + implicit TCC prompt | Mic permission is requested implicitly on first engine start | Windows 10+ mic privacy setting; no prompt API — must detect `AUDCLNT_E_...`/access-denied and surface guidance |
| `NSEvent.addGlobalMonitorForEvents` / `addLocalMonitorForEvents` (AppKit) | Global hotkey observation for `.flagsChanged`, `.keyDown`, `.keyUp` without consuming the event | `SetWindowsHookEx(WH_KEYBOARD_LL)` or `RegisterHotKey`. LL hook is the only way to get modifier press/release edges |
| `NSEvent.ModifierFlags` in `flagsChanged` | Infer press vs release of modifier-only keys (Ctrl/Opt/Cmd/Shift/Fn) that emit no key events | LL hook gives real `WM_KEYDOWN`/`WM_KEYUP` for `VK_LCONTROL` (0xA2) / `VK_RCONTROL` (0xA3) etc. — **simpler and more accurate** than macOS |
| `CGEvent.getIntegerValueField(.eventSourceUnixProcessID) == 0` | Reject synthetic keystrokes (esp. Universal Control re-posted modifier state from another Mac) so the hotkey only fires on physical local input | `KBDLLHOOKSTRUCT.flags & LLKHF_INJECTED` (and `LLKHF_LOWER_IL_INJECTED`) |
| `NSEvent.mouseLocation` + `NSScreen.screens` | Ignore hotkeys when the cursor is on a display not owned by this machine (Universal Control) | No Windows analogue needed; drop the check or keep a trivial always-true |
| `AXIsProcessTrustedWithOptions` (ApplicationServices) | Prompt for / check Accessibility permission required to synthesize input | No equivalent; UIPI/elevation is the analogue (can't inject into elevated windows from a non-elevated process) |
| `AXUIElementCreateSystemWide` + `kAXFocusedUIElementAttribute` / `kAXValueAttribute` / `kAXSelectedTextAttribute` | Read the focused control's text (transcription context) and read it back to verify a paste landed; read the current selection | UI Automation (`IUIAutomation::GetFocusedElement`, `TextPattern`/`ValuePattern`) |
| `NSPasteboard` (multi-item, multi-type snapshot/restore) | Save the full clipboard, put plain text, restore all items+formats afterwards | `OpenClipboard`/`EnumClipboardFormats`/`GetClipboardData` — Windows clipboard has one item with N formats, so the nested `[[(type, data)]]` collapses to a flat `[(format, data)]` |
| `CGEvent(keyboardEventSource:virtualKey:keyDown:)` + `post(tap: .cghidEventTap)` | Synthesize Cmd+V (vk 9) for paste, Cmd+C (vk 8) for polish, and Return (36) / Tab (48) in natural mode | `SendInput` with `KEYEVENTF_KEYUP`; Ctrl+V = `VK_CONTROL` + `V` (0x56), Ctrl+C = `V`→`C` (0x43), Return = `VK_RETURN`, Tab = `VK_TAB` |
| `CGEventSource(stateID: .privateState)` | Isolate synthesized events from live hardware modifier/Caps-Lock state so punctuation isn't corrupted | No equivalent — must explicitly clear/restore modifier state, or prefer `KEYEVENTF_UNICODE` which ignores modifiers entirely |
| `CGEvent.keyboardSetUnicodeString` | Type characters that have no key on the current layout (emoji, CJK) | `SendInput` with `KEYEVENTF_UNICODE` (surrogate pairs sent as two inputs) |
| `TISCopyCurrentKeyboardLayoutInputSource`, `TISGetInputSourceProperty`, `UCKeyTranslate`, `LMGetKbdType` (Carbon HIToolbox) | Build char → (virtualKey, modifiers) reverse map for the active layout so natural-mode keystrokes are indistinguishable from real typing | `GetKeyboardLayout` + `VkKeyScanEx` (gives vk + shift-state directly, much simpler) or just use `KEYEVENTF_UNICODE` |
| `CGWindowListCopyWindowInfo` + `CGWindowListCreateImage` | Find the frontmost app's normal-layer window and capture just that window's pixels | `GetForegroundWindow` + `PrintWindow`/`BitBlt`, or Windows.Graphics.Capture |
| `VNRecognizeTextRequest` (Vision) | On-device OCR of the captured window, fast level, no language correction, first 50 lines | Windows.Media.Ocr (`OcrEngine`) — on-device, comparable |
| `NSWorkspace.shared.frontmostApplication` | Frontmost app name + bundle id for context/classification | `GetForegroundWindow` → `GetWindowThreadProcessId` → process image path / AppUserModelID |
| `NSWorkspace.shared.runningApplications` | Detect whether Apple Music / Spotify is running | Process enumeration by executable name, or SMTC session enumeration |
| `NSAppleScript` (Apple Music / Spotify: query `player state`, `pause`, `play`) | Pause music while dictating and resume afterwards | `GlobalSystemMediaTransportControlsSessionManager` (SMTC): read `PlaybackStatus`, call `TryPauseAsync`/`TryPlayAsync` — works for **all** players, not just two |
| `NSWorkspace.willSleepNotification` | Abort an in-flight recording when the machine sleeps | `WM_POWERBROADCAST` / `PBT_APMSUSPEND` |
| `AVAudioPlayer`, `NSSound(named:"Tink"/"Pop")` | Play start/stop UI sounds; fall back to system sounds when the pack file is missing | `rodio`/XAudio2 for the WAVs; `PlaySound(SND_ALIAS)` or a bundled fallback for the system sounds (**note: "Tink"/"Pop" have no Windows equivalent — bundle a WAV**) |
| `NSAppleEventManager` `kAEGetURL` handler | `wispr-flow://auth/...` deep-link sign-in callback | Registry `URL Protocol` handler + single-instance IPC |
| `Bundle.main.url(forResource:subdirectory:)` | Locate sound packs inside the app bundle | Resource directory next to the exe / Tauri resource resolver |

---

## 10. Parity risks on Windows

1. **System-default-input mutation.** macOS changes the machine-wide default input device to honor the mic setting. On Windows you should bind the capture stream to a specific `IMMDevice`; this is *better* but means the observable side effect (other apps switching mics) disappears. Confirm that's acceptable — it also removes the “device disappeared → silently falls back to system default” path, which you must reimplement explicitly (`.startedWithFallback`).
2. **Modifier-only push-to-talk.** Left Control as a hotkey means every Ctrl-key chord in every app (`Ctrl+C`, `Ctrl+V`) starts a dictation. On macOS `flagsChanged` gives clean edges and the app relies on the 0.5 s tap/hold logic to make this bearable. A Windows LL hook sees `VK_LCONTROL` down + auto-repeat; you must suppress auto-repeat with the same `keyDown` latch, and decide whether to swallow the key (macOS does **not** — monitors are passive; the LL hook must return `CallNextHookEx` to preserve that).
3. **Left/Right modifier aliasing.** macOS cannot distinguish which Control is still held (shared flag bit). Windows can. Naive porting will produce *different* (more correct) behavior for “press L-Ctrl, press R-Ctrl, release L-Ctrl”. Decide explicitly.
4. **Injected-event rejection.** `LLKHF_INJECTED` is the analogue of the PID==0 check, but AutoHotkey/remapping tools and some KVMs set it. Rejecting injected events may break users with keyboard remappers who work fine on macOS.
5. **Universal Control guards** (`isCursorOnLocalDisplay`, PID check) have no Windows meaning — dropping them is fine, but they are load-bearing on macOS, so don't “port” them as a no-op that accidentally returns false.
6. **Input Monitoring silent failure.** The macOS app has no diagnostic when hotkeys are dead. On Windows the equivalent failure is UIPI: an unelevated app receives no keys from, and cannot `SendInput` into, an elevated window (Task Manager, elevated terminals). Users will hit this. Plan a detectable error + guidance — this is a *new* requirement, not a port.
7. **Clipboard model mismatch.** macOS pasteboards hold N items × M types; Windows holds 1 item × M formats. The save/restore snapshot must be flattened; restoring “N items” is impossible. Also Windows delayed-rendering formats (`CF_HDROP`, owner-rendered `CF_BITMAP`) cannot be round-tripped byte-for-byte; a clipboard save/restore will lose data for some sources. The 0.25 s restore delay is also a guess — clipboard-monitor apps (and Windows Clipboard History) will capture the transcription text.
8. **Paste verification via AX.** UI Automation `ValuePattern` is far less universally implemented than macOS AX, and Electron/Chromium apps often expose nothing. The macOS code's “assume success when unreadable” default means verification is effectively a no-op in many apps — replicate that permissiveness or paste verification will start reporting false failures.
9. **Natural Mode layout mapping.** `VkKeyScanEx` returns vk+shift-state directly, but dead keys and AltGr layouts behave differently than `UCKeyTranslate` with `kUCKeyTranslateNoDeadKeysBit`. Safest parity path: use `KEYEVENTF_UNICODE` for everything except `\n` (`VK_RETURN`) and `\t` (`VK_TAB`) — but that changes observability for apps that watch virtual key codes (games, IDE keybindings), which is exactly the property the macOS implementation was designed to preserve.
10. **Per-character timing.** Must reproduce `baseDelay = 1/cps` with `×random(0.6..1.4)` jitter **and** a separate `random(30..80) ms` key-hold. Both are user-visible speed characteristics. Naive `SendInput` batching would type instantly.
11. **Window capture + OCR.** `PrintWindow` fails or returns black for GPU-composited windows (Chrome, Electron, hardware-accelerated apps) — precisely the apps users dictate into. Windows.Graphics.Capture is more reliable but shows a yellow capture border by default (needs `IsBorderRequired = false`, Windows 11 build 22000+). Windows.Media.Ocr requires the language pack to be installed; accuracy and line segmentation will differ from Vision, so the 50-line cap yields different context. Also note: **there is no timeout on OCR today** — add one, since Windows OCR on a 4K window can take seconds.
12. **Music control.** SMTC covers far more apps than the hardcoded Apple Music + Spotify pair, so Windows behavior will be a superset. Preserve the “only resume what we paused” flag logic, and fix the documented race (resume firing before pause completes on very short recordings) — the async pause is genuinely buggy.
13. **Frontmost-app classification.** Windows has no bundle IDs. You need a mapping from exe name / AUMID to the same `"messaging"|"email"|"ai"|"other"` labels, and the server sees `bundle_id` — decide what to send (exe name? synthetic id?) since the transcription API keys personalization off it.
14. **Trailing 640-sample remainder is dropped** in `processBuffer`. Fixing this in Rust changes the audio sent upstream (slightly longer, no micro-gaps). Almost certainly an improvement, but it is a behavioral difference.
15. **No live audio meter exists.** If the new overlay shows a waveform, that is a *new feature*, not parity — the current overlay only knows elapsed seconds and coarse states.
16. **Dead code/assets to not blindly port**: `playPaste()` is never called; `paste.wav`, `achievement.wav`, `Notification.wav`, `popo-lock.wav` are unreferenced; `TextInjector.readSelectedText()` is unused (polish uses synthetic Ctrl/Cmd+C instead); `AppInfoDetector`'s `url` field is always `""`.
17. **Clipboard leak on CGEvent creation failure** in `pasteViaClipboard`: the early `completion(false)` return skips the restore, permanently clobbering the user's clipboard. Fix in the port.
18. **The mic stays hot after every recording** (`stop()` forces `isPrewarmed = true`) regardless of `keepMicrophoneActive`. On Windows this keeps the mic-in-use privacy indicator lit and may block other apps in exclusive mode. Same rationale (avoid Bluetooth/endpoint renegotiation) applies, but the user-visible privacy indicator is more prominent on Windows 11.