# Wispr Lightning — Swift to Rust/Tauri Parity Matrix

This is the historical parity ledger for the original Swift-to-Rust port. The
Deepgram-only cutover documented in `docs/PORT_PLAN.md` supersedes every row
that requires Wispr Flow, OpenRouter, Claude Voice, OAuth/session handling,
AI Polish, provider selection, provider capabilities, or a fallback chain.
Those rows remain below as source archaeology; they are not live requirements.

Current release gates and observed results are in `docs/PORT_PLAN.md`.

Column contract:

- **ID** — stable and permanent. Never renumber; retire an ID rather than reuse it.
- **Behavior** — trigger to observable result, with the exact literals (timings, keycodes, pixel
  sizes, JSON keys, SQL, verbatim UI and log strings) that the implementation must reproduce.
- **Swift source** — the reference file. Line numbers appear only where the specs give one.
- **Owner** — the target crate/module from PORT_PLAN.md section 3.
- **macOS / Windows** — `same` when the behavior is identical, `n/a` when it does not exist on that
  platform, `DEVIATION DVn` where PORT_PLAN.md section 2 says we intentionally differ (a retired-as-dead-code
  row keeps its ID, carries its DVn and takes `Verify = n/a`), `DECISION Dn`
  where PORT_PLAN.md section 2 fixes the mechanism, otherwise the concrete platform approach.
- **Verify** — which of the five verification layers proves it:
  `fixture` (5.1 golden fixtures from the Swift reference), `unit` (5.2 `wl-core` host-agnostic),
  `contract` (5.3 provider contract suite over a mock server), `probe` (5.4 `wl-platform` probe on a
  real OS), `e2e` (5.5 Tauri smoke), `manual` (last resort, with the exact step named inline).

## Status summary

Reconciled against a full workspace run, a §5.4 platform-probe run on real hardware, the macOS and
Windows thread/apartment audits, and the ui/ bundle runs — all verified by Main or reported with
method: `cargo test --workspace` 541 passing / 0 failed (wl-core 171 including 9 fixture-parity,
wl-providers 138, wl-platform 97, src-tauri 106), clippy 0 warnings, `cargo fmt --check` clean,
`pnpm check` 299 files 0 errors, `cargo xwin check --workspace --all-targets` clean.

| Status | Rows | Share |
|---|---|---|
| `done` | 664 | 92.9% |
| `todo` | 32 | 4.5% |
| `blocked` | 15 | 2.1% |
| `n/a` (deliberately not ported) | 4 | 0.6% |
| **Total** | **715** | **100.0%** |

> ### NOT VERIFIED ON WINDOWS HARDWARE
> Every Windows claim in this file rests on source audit and cross-compile type-checking
> (`cargo xwin check --workspace --all-targets --target x86_64-pc-windows-msvc`, clean) — **not on
> execution**. No line of this application has been run on Windows. **103 rows are marked
> `done` on macOS evidence while their Windows column describes a distinct, never-executed
> implementation**; the 15 `blocked` rows are only those that could not be exercised
> on macOS *at all*, so `blocked` understates Windows exposure roughly 6-fold.
> Read `done` as "proven on macOS, type-checks on Windows".
>
> The Windows apartment bug found by source audit alone — `ShellExecuteW` from the MTA silently
> failing to open the microphone settings page — is what that gap looks like in practice: a real
> defect on a real user path, invisible to every green check in the table above.

`todo` means no evidence has been produced yet, not that the code is missing. Every remaining `todo`
and `blocked` row carries its reason inline in the Status cell. `n/a` is a fourth status, not a
tidier form of `done`: those rows are the DV9, DV10 and DV12 dead-code retirements, which will never
be done and for which no work is pending.

## How a row gets closed

**A test that closes a matrix row cites that row's ID — in the test name, or in a comment directly
above it.** This is a standing rule in PORT_PLAN section 5, not a suggestion, and it exists so
reconciliation is a grep rather than a judgement call.

It was adopted because it was already happening: **103 distinct row IDs are cited in Rust source
today**, put there by agents unprompted. That is the strongest argument that it is the natural
convention rather than an imposed one.

> **The back catalogue does not follow it.** The 428 rows closed under the earlier rule — Verify is
> `unit`, `fixture` or `contract` and the owning crate is green — predate the convention. Only 12 of
> them cite a row ID anywhere in source and only 6 appear in a test file, so **a clean grep over this
> matrix does NOT mean clean coverage**. Roughly 357 of those rows can be neither confirmed nor
> refuted mechanically.

The reason is structural, not sloppiness: the Behavior column cites **Swift** symbols by design,
because the Swift sources are the reference implementation and the fixture oracle, while the port
carries Rust names. The two do not share a vocabulary, so no identifier match can bridge them. That
is why all three false greens found so far — AUT-019, LIF-007 and HTK-010 — were caught by agents
reading source for unrelated reasons rather than by any audit. Luck does not scale.

*One trap for the next person auditing:* search `.ts` and `.svelte` as well as `.rs`. A first pass
here flagged SET-054 as unimplemented when its literals live in `ui/src/lib/ipc.ts` and
`Dictation.svelte`, with the Rust half unit-tested at `settings.rs:603`; HTK-014 was flagged the same
way when its latch is `edge_from` in `crates/wl-platform/src/windows/matching.rs`, tested at lines
529-541. Both were false positives caused by loading only the Rust corpus.

## Sections

1. [Audio capture](#1-audio-capture) — 38 rows
2. [Hotkeys & recording state machine](#2-hotkeys--recording-state-machine) — 50 rows
3. [Text injection](#3-text-injection) — 41 rows
4. [Context capture (frontmost app, AX/UIA text, screen OCR)](#4-context-capture-frontmost-app-axuia-text-screen-ocr) — 20 rows
5. [Media control & sound cues](#5-media-control--sound-cues) — 23 rows
6. [Transcription protocol (Wispr WSS)](#6-transcription-protocol-wispr-wss) — 68 rows
7. [Provider abstraction & Deepgram (NEW functionality)](#7-provider-abstraction--deepgram-new-functionality) — 31 rows
8. [Polish (manual hotkey flow + auto-polish)](#8-polish-manual-hotkey-flow--auto-polish) — 39 rows
9. [Auth & session](#9-auth--session) — 39 rows
10. [Persistence (SQLite schema, every store query, settings JSON)](#10-persistence-sqlite-schema-every-store-query-settings-json) — 62 rows
11. [Dictionary & auto-learn](#11-dictionary--auto-learn) — 28 rows
12. [Tray / menu bar](#12-tray--menu-bar) — 22 rows
13. [Recording overlay](#13-recording-overlay) — 43 rows
14. [Settings UI (one row per control)](#14-settings-ui-one-row-per-control) — 119 rows
15. [History / Notes / Dictionary windows](#15-history--notes--dictionary-windows) — 54 rows
16. [App lifecycle](#16-app-lifecycle) — 25 rows
17. [Logging & diagnostics](#17-logging--diagnostics) — 13 rows

---

## 1. Audio capture

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|AUD-001|Recording capture graph taps the default input node at bus 0 in the hardware's native format (arbitrary rate, float32, N channels) rather than requesting a fixed format from the OS.|AudioRecorder.swift|wl-platform::audio|cpal input stream on CoreAudio default/selected device|cpal input stream on WASAPI shared-mode capture client|probe|done|
|AUD-002|Tap buffer size is requested as 640 frames (`chunkSamples`); the OS may deliver larger buffers and the chunker must tolerate that.|AudioRecorder.swift|wl-platform::audio|cpal buffer-size hint 640 frames|cpal buffer-size hint 640 frames|probe|done|
|AUD-003|Target capture format is exactly 16000 Hz, 1 channel, signed 16-bit little-endian, interleaved.|AudioRecorder.swift|wl-core::packetizer|same|same|unit|done|
|AUD-004|Format converter is cached and reused only when BOTH inputFormat == hwFormat AND outputFormat == targetFormat; otherwise it is recreated.|AudioRecorder.swift|wl-platform::audio|rubato::Fft resampler rebuilt on hw-format change|rubato::Fft resampler rebuilt on hw-format change|unit|done|
|AUD-005|Per-buffer conversion computes `ratio = 16000 / hwSampleRate` and allocates output capacity `frameLength * ratio` truncated to an integer frame count, performing resample + downmix + int16 quantize in one pass.|AudioRecorder.swift|wl-platform::audio|DECISION D7 — downmix then rubato::Fft FixedSync::Output 640 on worker thread|DECISION D7 — downmix then rubato::Fft FixedSync::Output 640 on worker thread|unit|done|
|AUD-006|Resampling never runs on the realtime capture callback; converted frames reach the worker through an rtrb SPSC ring buffer.|AudioRecorder.swift|wl-platform::audio|DECISION D7 (Swift converts inline on the tap thread)|DECISION D7 (Swift converts inline on the tap thread)|unit|done|
|AUD-007|A conversion that returns a non-nil error or a zero-length output buffer is dropped silently with no log and no packet emitted.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|AUD-008|The converted Int16 buffer is sliced into packets of exactly 640 samples (1280 bytes) while `offset + 640 <= totalSamples`.|AudioRecorder.swift|wl-core::packetizer|same|same|fixture|done|
|AUD-009|The sub-640-sample tail of every converted buffer is carried into a remainder ring buffer instead of being discarded, so no audio is lost between callbacks.|AudioRecorder.swift|wl-core::packetizer|DEVIATION DV1 (Swift discards up to 40 ms per callback)|DEVIATION DV1 (Swift discards up to 40 ms per callback)|unit|done|
|AUD-010|The packet vector is mutated under a lock and the capture callback early-returns unless `isRecording` is true, so a pre-warmed tap produces no packets.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|AUD-011|`mic_device_id == None` selects the system default input and `selectConfiguredDevice()` returns true immediately without touching any device; `mic_device_name` is never consulted to make this decision.|AudioRecorder.swift|wl-platform::audio|same|same|probe|done|
|AUD-012|Choosing a specific microphone binds the capture stream to that device instead of rewriting the machine-wide default input.|AudioRecorder.swift|wl-platform::audio|DEVIATION DV2 (Swift sets kAudioHardwarePropertyDefaultInputDevice)|DEVIATION DV2; bind IMMDevice endpoint per stream|probe|todo — probe recorded on the system default; binding to a chosen device not exercised|
|AUD-013|Device identity is persisted as TWO fields with distinct jobs: `mic_device_id: Option<String>` (`coreaudio:<uid>` or `wasapi:<endpoint>`, None = system default) is the SOLE resolution key and the only value ever compared against enumerated devices, and `mic_device_name: Option<String>` is a display label only. Resolving a device by name is forbidden; storing the name is not.|AudioRecorder.swift|wl-core::settings|DECISION D6 cpal DeviceTrait::id()|DECISION D6 cpal DeviceTrait::id()|unit|done|
|AUD-014|A cached (uid, deviceID) pair short-circuits device lookup; on a failed direct set the cache is invalidated and the full enumeration path runs.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|AUD-015|Slow path enumerates all devices, string-compares each device UID against the requested UID, and populates the cache on match.|AudioRecorder.swift|wl-platform::audio|same|enumerate IMMDeviceEnumerator ids|probe|done|
|AUD-016|Requested microphone absent -> log `Wispr Lightning: Requested mic '<name>' not available, using system default` where the name is read from the stored `mic_device_name` display hint (NOT from a name lookup, which would be a resolution by name and is forbidden), `selectConfiguredDevice()` returns false, `start()` returns `.startedWithFallback`, and recording proceeds on the system default with no user-visible UI. Keeping the stale name is what lets the message say `Yeti Nano is not connected` instead of `unknown device`.|AudioRecorder.swift|wl-platform::audio|same|same (explicit fallback must be reimplemented, not free)|probe|todo — probe layer not yet run for this capability|
|AUD-017|On `.startedWithFallback` the orchestrator logs `Recording started with fallback mic (requested device unavailable)` and shows nothing to the user.|AppDelegate.swift|src-tauri::orchestrator|same|same|unit|done|
|AUD-018|`listInputDevices()` returns (uid, name) pairs only for devices whose input-scope stream configuration reports at least one buffer with at least one channel.|AudioRecorder.swift|wl-platform::audio|same|IMMDeviceEnumerator eCapture + PKEY_Device_FriendlyName|probe|done|
|AUD-019|Two DISTINCT system-wide device listeners are registered — one for the device list, one for the default input — each invalidating the resolved device and surfacing through `AudioCapture::take_faults()` so the next prewarm or start re-resolves.|AudioRecorder.swift|wl-platform::audio|crates/wl-platform/src/macos/devices.rs: two AudioObjectAddPropertyListener registrations on kAudioObjectSystemObject, global scope / main element, distinct boxed client-data each, added and removed on one dedicated thread|crates/wl-platform/src/windows/devices.rs: IMMNotificationClient via RegisterEndpointNotificationCallback; OnDefaultDeviceChanged filtered to eCapture+eConsole, OnDeviceAdded/Removed/StateChanged for list changes, OnPropertyValueChanged ignored; joins the process-wide implicit MTA and never calls CoInitializeEx|probe|done|
|AUD-020|A capture-stream reconfiguration invalidates the resolved device and posts an audio-devices-changed event, so the next prewarm or start re-resolves. The Swift original also logs `Wispr Lightning: AVAudioEngine configuration changed`; there is no AVAudioEngine in the port, so that literal string has no analogue and is deliberately NOT a parity assertion — the observable behaviour is.|AudioRecorder.swift|wl-platform::audio|cpal per-stream error callback: ErrorKind::DeviceChanged -> CaptureFault::DefaultChanged, StreamInvalidated -> CaptureFault::StreamInvalidated|same mapping; the system default-input listener fires for the same event so on_faults re-resolves either way|probe|done|
|AUD-021|On audio-devices-changed while recording, the status-bar menu refreshes and, if the configured UID no longer exists, `Target mic '<name>' disconnected during recording` is logged — recording is NOT stopped, packets simply stop arriving.|AppDelegate.swift|src-tauri::orchestrator|same|same|unit|done|
|AUD-022|On audio-devices-changed while NOT recording, `rearmMicrophone()` runs.|AppDelegate.swift|src-tauri::orchestrator|same|same|unit|done|
|AUD-023|`rearmMicrophone()` debounces on a 0.15 s timer, then deactivates the mic and re-prewarms only if `keepMicrophoneActive` is true; it also fires on any settings change.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|AUD-024|At launch, `keepMicrophoneActive == true` triggers `prewarm()`.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|AUD-025|`prewarm()` is a no-op when already prewarmed or recording; otherwise it selects the device, installs the tap, starts the engine, sets `isPrewarmed = true` and logs `Wispr Lightning: Microphone pre-warmed (input: %@)`. On throw it logs the failure and removes the tap.|AudioRecorder.swift|wl-platform::audio|same|same|probe|done|
|AUD-026|While pre-warmed the tap is installed but every buffer is discarded because `isRecording` is false, and the OS mic-in-use indicator stays lit.|AudioRecorder.swift|wl-platform::audio|same|same; Windows 11 privacy indicator is more prominent|manual: observe the OS microphone indicator with keepMicrophoneActive on and no dictation running|todo — manual step not yet performed|
|AUD-027|`deactivate()` runs only when prewarmed and not recording: removes the tap, stops the engine, clears `isPrewarmed`, logs `Wispr Lightning: Microphone deactivated`.|AudioRecorder.swift|wl-platform::audio|same|same|probe|todo — probe layer not yet run for this capability|
|AUD-028|`stop()` always sets `isPrewarmed = true` and leaves the engine running regardless of `keepMicrophoneActive`, so the mic stays hot after the first dictation until a settings or device change re-arms it.|AudioRecorder.swift|wl-platform::audio|same (avoids Bluetooth renegotiation)|same (keeps WASAPI stream open, avoids endpoint renegotiation)|probe|done|
|AUD-029|`start()` returns `.started` immediately with log `Recording started (prewarmed mic)` when prewarmed and the engine is running.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|AUD-030|`start()` with `isPrewarmed` true but a dead engine removes the tap, clears `isPrewarmed`, and falls through to the full start path.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|AUD-031|Successful engine start logs `Audio engine started (input: %@, rate: %.0f Hz)` and returns `.startedWithFallback` when the configured device was unavailable, else `.started`.|AudioRecorder.swift|wl-platform::audio|same|same|probe|done|
|AUD-032|Engine start throw -> log, tap removed, `isRecording = false`, `.failed(localizedDescription)`; the orchestrator then shows overlay error `Mic unavailable`, resets state to idle and resumes music.|AudioRecorder.swift|wl-platform::audio|same|same|e2e|done|
|AUD-033|`stop()` clears `isRecording`, snapshots the packet list under the lock, and logs `Recording stopped — %d packets (%.1fs)` where duration is `count * 40 / 1000`.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|AUD-034|`cleanup()` removes the tap, stops the engine and drops the cached converter.|AudioRecorder.swift|wl-platform::audio|same|same|probe|todo — probe layer not yet run for this capability|
|AUD-035|No explicit microphone permission API is called; the TCC prompt is raised implicitly by the first engine start using `NSMicrophoneUsageDescription` = `Wispr Lightning needs microphone access to record your voice for dictation.`|AudioRecorder.swift|wl-platform::permissions|same (Info.plist usage string + hardened-runtime entitlement)|No prompt API: detect E_ACCESSDENIED surfacing as cpal BackendError and deep-link ms-settings:privacy-microphone|probe|done|
|AUD-036|No audio level, amplitude, waveform or RMS is published during recording; the overlay receives only elapsed seconds and state changes.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|AUD-037|A capture stream that returns only all-zero samples is detected and surfaced rather than silently producing a silent transcript.|n/a (new)|wl-platform::audio|not present today; guard added for parity of failure reporting|Windows silent-capture failure mode explicitly guarded|probe|done|
|AUD-038|`CaptureFault::DefaultChanged` and `CaptureFault::DevicesChanged` mean different things and must not be collapsed: DefaultChanged says the default input moved, so a default-bound stream is rebuilt; DevicesChanged says the device SET changed, so the picker is refreshed and the mic re-armed but a healthy stream is NOT rebuilt — a microphone appearing elsewhere on the machine says nothing about the stream currently held. Both are non-terminal, unlike DeviceLost and StreamInvalidated.|n/a (new)|wl-platform::audio|same|same; macOS reroutes a default-bound stream transparently and Windows does not, which is why DefaultChanged exists separately|unit|done|

## 2. Hotkeys & recording state machine

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|HTK-001|Global hotkey observation is passive and never consumes the keystroke: three monitors are installed — global `.flagsChanged`, local `.flagsChanged` (returns the event unmodified), global `[.keyDown, .keyUp]`.|HotkeyListener.swift|wl-platform::macos::hotkey|handy-keys 0.3.3 backend; CGEventTap must be re-enabled inside the callback on TapDisabledByTimeout/ByUserInput|WH_KEYBOARD_LL on a dedicated message-pump thread; hook proc returns CallNextHookEx so the key is not swallowed|probe|done|
|HTK-002|Only these keycodes have pre-named labels: 59 Left Control, 62 Right Control, 58 Left Option, 61 Right Option, 55 Left Command, 54 Right Command, 56 Left Shift, 60 Right Shift, 63 Fn, 36 Return, 49 Space, 53 Escape, 48 Tab.|HotkeyListener.swift|wl-core::settings|same|Portable Chord enum; there is no Fn virtual key on Windows so that row is dropped|unit|done|
|HTK-003|The modifier keycode set used for press/release branching is exactly {59, 62, 58, 61, 55, 54, 56, 60, 63}.|HotkeyListener.swift|wl-platform::macos::hotkey|same|VK_LCONTROL 0xA2, VK_RCONTROL 0xA3, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN, VK_LSHIFT, VK_RSHIFT; no Fn|unit|done|
|HTK-004|`rebuildHotkeySet()` runs on settings-changed, on `start()` and on `rebind()`: dictation set = `hotkeyKeyCodes` when non-empty else `{hotkeyKeyCode}`; polish set = `polishHotkeyKeyCodes`.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-005|`hotkeyKeyCodes` is a set of independent alternative triggers (press A OR B), never a simultaneous chord; there is no combo matching anywhere.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-006|Listener startup logs `Hotkey listener active (press <labels joined by " or ">to dictate)`.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-007|`rebind(keyCode:)` removes monitors, sets `hotkeyKeyCode`, `hotkeyLabel = keycodeLabels[k] ?? "Key <k>"`, `hotkeyKeyCodes = [k]`, `hotkeyLabels = [label]`, saves settings, rebuilds the set and reinstalls the monitors.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-008|Modifier press vs release is inferred from the flags in the flagsChanged event: 59/62 -> `.control`, 58/61 -> `.option`, 55/54 -> `.command`, 56/60 -> `.shift`, 63 -> `.function`, anything else -> false.|HotkeyListener.swift|wl-platform::macos::hotkey|same|LL hook gives real WM_KEYDOWN/WM_KEYUP per side; no flag inference needed|unit|done|
|HTK-009|Left and right variants share a flag bit, so holding Left Control then pressing Right Control keeps the flag set and releasing one while the other is held is NOT seen as a release.|HotkeyListener.swift|wl-platform::macos::hotkey|same (physical limitation of flagsChanged)|Windows distinguishes sides; the aliasing quirk is intentionally not reproduced|unit|done|
|HTK-010|Every trigger is gated on `isCursorOnLocalDisplay()` — the pointer must be inside one of this machine's screens — so a Universal Control cursor on another Mac suppresses the hotkey.|HotkeyListener.swift|wl-platform::macos::hotkey|same|n/a — no Universal Control; guard is compiled out, never stubbed to a value that can return false|unit|todo — NO IMPLEMENTATION: confirmed absent from crates/ and src-tauri/src/. Main has commissioned the fix — roughly ten lines, NSScreen.screens containing NSEvent.mouseLocation, checked on the press path only; macOS-only, and the Windows guard must be COMPILED OUT rather than stubbed to anything that can return false|
|HTK-011|SELF-INJECTED keystrokes are rejected. `begin_synthetic_input` / `synthetic_input_in_flight` arm a 150 ms `SYNTHETIC_GUARD` around every event the injector posts (Cmd+V paste, polish Cmd+C, Natural Mode per character), honoured at the top of `Worker::on_key_event`, and every synthesized event carries `kCGEventSourceUserData = 0x574C4921`. This fixed a REAL bug on the default path: Natural Mode typing a character was read back by the tap as a fresh press, reproduced live as `UNGUARDED: hotkey fired -> HotkeyEvent { binding: Dictate, transition: Pressed }` versus `ARMED: no hotkey event`. No test in the suite would have caught it.|HotkeyListener.swift|wl-platform::macos::hotkey|DEVIATION DV13 (closed half): 150 ms guard plus user-data tag|DEVIATION DV13 (closed half): same mechanism, same 150 ms window, same 0x574C4921 tag via dwExtraInfo|probe|done|
|HTK-050|FOREIGN-PROCESS synthetic keystrokes are ACCEPTED, not rejected. The Swift original requires `kCGEventSourceUnixProcessID == 0`; `handy-keys` 0.3.3 owns the `CGEvent` and surfaces only keycode and flags, so the PID is unreachable at our layer (verified against the crate; 0.3.3 is the latest published version). Two workarounds were rejected with sound reasoning: a second listen-only tap cannot suppress anything in the first, and an NSEvent monitor fires AFTER OS dispatch while a tap fires before it, so its answer always arrives too late. Accepted for a second and independent reason: it is what lets Karabiner and other remappers keep working, and the Windows side already deliberately allows them (Open questions Q9) — so the two platforms are now consistent with each other, which matters more here than matching the original exactly. REVISIT TRIGGER: upstream exposing the field, or us owning the tap. The pure predicate `accepts_key_event(source_pid, self_injecting)` is already written and unit-tested against the real Swift semantics, so it is a one-argument change the day either happens.|HotkeyListener.swift|wl-platform::macos::hotkey|DEVIATION DV13 (accepted half): foreign synthetic input is allowed through|DEVIATION DV13 (accepted half): LLKHF_INJECTED readable but deliberately not rejected, per Q9|n/a|n/a|
|HTK-012|Dictation press fires only when `pressed && !keyDown && onScreen && localHID && !isPaused`, setting `keyDown = true` and `activeKeyCode = keycode` before invoking the press handler.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-013|Dictation release fires when `!pressed && keyDown && activeKeyCode == keycode`, and is deliberately NOT gated on onScreen, localHID or isPaused, so a locally started recording always receives its release.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-014|Non-modifier hotkeys use the keyDown/keyUp path with identical latch logic plus `guard !isModifierKeycode(event.keyCode)`.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-015|There is no debounce and no repeat suppression beyond the `keyDown` latch, which naturally swallows auto-repeat key-down events.|HotkeyListener.swift|wl-platform::macos::hotkey|same|LL hook sees VK_LCONTROL auto-repeat; the same latch must suppress it|unit|done|
|HTK-016|Every hotkey evaluation emits a verbose line of the form `Hotkey[global-flags] keycode=59 pressed=true onScreen=true localHID=true paused=false keyDown=false`.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same, with path label `ll-hook`|unit|done|
|HTK-017|`resetState()` and `removeMonitors()` both clear `keyDown` and `activeKeyCode`.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-018|Press while `.idle` -> `state = .listening`, `lastPressTime = now`, `startRecordingSession()`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-019|Press while `.listening` with `now - lastPressTime < 0.5 s` -> `state = .recording` (hands-free lock), `lastPressTime = now`, log `Recording locked — hands-free mode`, `overlay.showLocked()`; the pending tap timer is cancelled first.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-020|Press while `.listening` with `now - lastPressTime >= 0.5 s` -> `stopRecordingSession()`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-021|Press while `.recording` (locked) -> `stopRecordingSession()`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-022|Release is ignored unless `state == .listening`; in locked mode a release does nothing.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-023|Release with `heldDuration >= 0.5 s` schedules a one-shot 0.5 s trailing-buffer timer that stops the session if still `.listening`; `heldDuration` defaults to 1.0 when `lastPressTime` is nil.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-024|Release with `heldDuration < 0.5 s` schedules a one-shot timer at `0.5 - heldDuration`, so a quick tap auto-stops exactly 0.5 s after the FIRST press.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-025|`hotkeyPaused` is read from settings, persisted in settings.json and therefore survives relaunch.|HotkeyListener.swift|wl-core::settings|same|same|unit|done|
|HTK-026|`setPaused(_:)` is a no-op when unchanged; otherwise it writes and saves settings, logs `Hotkey paused` or `Hotkey resumed`, and clears `keyDown`/`activeKeyCode` so a physically held key is not stuck across the toggle.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-027|While paused, both dictation and polish PRESS handlers early-return, but release handlers still run.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-028|The polish keycode is evaluated BEFORE the dictation check and only when it is in the polish set and NOT in the dictation set.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-029|Polish is press-only and edge-triggered: modifier-down (or `.keyDown` for regular keys) fires it, with no release handling, no hold semantics and no lock mode.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-030|`triggerPolish()` ignores any trigger occurring within 0.5 s of the previous accepted trigger.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-031|Polish requires `settings.polishEnabled` and a non-nil press handler, and the handler is dispatched asynchronously onto the main queue.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same|unit|done|
|HTK-032|Denied Input Monitoring produces a live monitor object but zero events: the app appears alive and simply never triggers, with no user-facing diagnostic.|HotkeyListener.swift|wl-platform::permissions|same behavior reproduced; permission status is surfaced via Permissions::status()|UIPI is the analogue — an unelevated process receives no keys from elevated windows; this is a NEW detectable error plus guidance|probe|todo — probe layer not yet run for this capability|
|HTK-033|A repeating 1.0 s timer runs while recording and computes `elapsed = Int(now - recordingStartTime)`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-034|`elapsed >= 600` -> log `Max recording duration reached (600s), auto-stopping` -> `stopRecordingSession()`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-035|`elapsed >= 570` (and below 600) -> `recordingOverlay.showFinalWarning()`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-036|`elapsed >= 540` (and below 570) -> `recordingOverlay.showWarning()`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-037|Every tick calls `recordingOverlay.updateElapsed(elapsed)` regardless of warning state.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-038|After stop, a recording with fewer than 5 packets (under 200 ms) is discarded and music is resumed.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-039|Zero packets with elapsed greater than 1.0 s -> overlay error `Mic not responding` plus a log noting a likely mic disconnect; otherwise log `Too short (N packets), ignoring` and hide the overlay.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-040|Press sequence order is fixed and asserted inline as ForegroundRead, Cue(Start), CaptureStart, Indicator(true), Overlay(Recording), Tick, with each blocking job indexed after CaptureStart against a 150 ms device-open window. The provider prewarm's POSITION within this sequence is deliberately NOT claimed: a task spawned from a worker parks in that worker's non-stealable LIFO slot, so the handshake cannot log until the actor yields no matter where the spawn sits — verified by moving it to the first statement of `start_recording` with the test still passing. That prewarm happens at all and overlaps the recording is proven separately by WSS-003.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|HTK-041|Stop sequence order is fixed: state to idle, invalidate timers, `audioRecorder.stop()`, playStop, status bar off, min-length gate, overlay showProcessing, save raw PCM to disk, arm the processing timeout, transcribe.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|HTK-042|The UI-level processing timeout is `max(30.0, 30.0 + recordingDuration * 0.5)` seconds and, on expiry, presents the retryable-error UI.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-043|Retryable transcription errors are retried automatically with a 1.5 s backoff between attempts.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|HTK-044|Persisted `hotkeyKeyCodes: [UInt16]` holding Carbon virtual keycodes migrates through a Carbon-to-portable table into the `Chord` enum stored by the port.|Settings.swift|wl-core::settings|Carbon codes map 1:1 to the portable Chord enum|Carbon codes remapped to VK_* equivalents; Fn (63) has no target and is dropped|unit|done|
|HTK-045|A Windows low-level hook that exceeds `LowLevelHooksTimeout` is silently removed with no notification; the hook proc therefore only sends on a channel and a liveness heartbeat reinstalls it.|n/a (new)|wl-platform::windows::hotkey|n/a|channel-send-only hook proc plus reinstall heartbeat|probe|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|HTK-046|`HotkeyBackend::begin_capture()` puts the backend into capture mode; `end_capture() -> Option<Hotkey>` leaves it and yields what was pressed. Capture is a backend concern, not a UI one, because a webview `keydown` never receives the macOS Fn key and both Fn and bare modifiers are bindable. Every arm MUST be matched by a disarm — a capture left armed leaves the backend suppressing the real hotkey handler (HTK-047), so the dictation hotkey would silently stop working with no error anywhere.|SettingsWindow.swift|wl-platform::hotkey|local NSEvent monitor for keyDown and flagsChanged, press edges only|WH_KEYBOARD_LL in capture mode, press edges only|unit|done|
|HTK-047|While capture is active, key presses MUST NOT emit `HotkeyEvent`s. Binding a new key must never start a recording — that is both wrong and alarming to the user.|SettingsWindow.swift|wl-platform::hotkey|same|same|e2e|done|
|HTK-048|`end_capture()` returning `None` means nothing usable was pressed and is treated as CANCELLED; it must never be signalled by returning an empty or otherwise invalid `Hotkey`, and it must never be read as an instruction to clear the existing binding.|SettingsWindow.swift|wl-platform::hotkey|same|same|unit|done|
|HTK-049|A `null` result from `hotkey_capture_end` leaves the existing binding COMPLETELY untouched in the settings UI — no write, no clear, no visual change beyond the capture button reverting from `Press a key…`. Cancel and the 15 s capture timeout take the SAME branch, so proving one proves both. The only two paths that can reach the settings writer from `KeyCapture` are the duplicate-checked add (SET-030) and the explicit remove (SET-026). `KeyCapture.svelte` is shared by the dictation and polish lists, so this one component covers both surfaces (SET-028 and SET-068) — and a regression would hit both at once rather than leaving one silently worse than the other.|SettingsWindow.swift|ui/settings|same|same|e2e|done|

## 3. Text injection

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|INJ-001|`inject(text:)` with an empty string immediately calls back false and does nothing else.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-002|Injection logs `TextInjector.inject called with N chars` before doing any work.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-003|All injection work runs on a single dedicated serial queue named `com.wisprlightning.textinjection`, never on the UI thread.|TextInjector.swift|wl-platform::inject|same (dedicated injection thread)|same (dedicated injection thread)|unit|done|
|INJ-004|A fixed 10 ms sleep precedes any synthesized input so the hotkey release is fully processed by the OS first.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-005|Strategy is chosen purely by `naturalModeEnabled`: true -> character-by-character typing, false -> clipboard paste. It is a switch, not a fallback chain.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-006|There is no accessibility write path: the AX API is used only for reading context, reading the selection and verifying a paste.|TextInjector.swift|wl-platform::inject|same|UIA is likewise read-only here; no ValuePattern::SetValue write path|unit|done|
|INJ-007|`saveClipboard()` snapshots every pasteboard item and every type on each item as (type, data) pairs on the main thread, skipping items with no readable data.|TextInjector.swift|wl-platform::clipboard|same (nested [[(type, Data)]])|Windows holds one item with N formats, so the nested structure flattens to [(format, data)]; delayed-render formats such as CF_HDROP cannot round-trip|unit|done|
|INJ-008|The clipboard is cleared and the transcript is set as plain string on the main thread synchronously before any key is synthesized.|TextInjector.swift|wl-platform::clipboard|same|same|unit|done|
|INJ-009|Log `Clipboard set, simulating Cmd+V` is emitted immediately before the paste keystroke.|TextInjector.swift|wl-platform::inject|same|log text becomes `simulating Ctrl+V`|unit|done|
|INJ-010|Paste is synthesized as virtual key 9 (`V`) down+up from a hid-system-state event source with `.maskCommand` on both events, posted to the HID event tap.|TextInjector.swift|wl-platform::inject|same|SendInput VK_CONTROL + 0x56 down/up pairs|probe|done|
|INJ-011|If the paste key event cannot be created, log `Failed to create Cmd+V CGEvent — check Accessibility permissions`, call back false, and STILL restore the clipboard.|TextInjector.swift|wl-platform::clipboard|DEVIATION DV4 (Swift leaks the transcript into the clipboard on this path)|DEVIATION DV4 (restore on all paths)|unit|done|
|INJ-012|Log `Cmd+V posted` after the key events are on the tap.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-013|A fixed 50 ms sleep follows the paste to let the target app consume it, before verification runs.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-014|Clipboard restore is scheduled on the main queue 0.25 s after the paste and, when the snapshot is non-empty, logs `Clipboard restored (%d items)`.|TextInjector.swift|wl-platform::clipboard|same|same; note Windows Clipboard History will already have captured the transcript|unit|done|
|INJ-015|Restore clears the pasteboard and re-creates one item per saved item, setting each saved type, writing items one at a time so ordering is preserved.|TextInjector.swift|wl-platform::clipboard|same|single item restored with all captured formats; N-item restore is impossible|unit|done|
|INJ-016|The completion value is the paste-verification result; a false result additionally logs `Paste verification failed — clipboard still restored`.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-017|Paste verification with no focused UI element logs that it is assuming success and returns true.|TextInjector.swift|wl-platform::inject|same|UIA GetFocusedElement returning nothing behaves identically|unit|done|
|INJ-018|Paste verification where the focused element's value attribute is missing or not a string logs that it could not read it and returns true.|TextInjector.swift|wl-platform::inject|same|same permissiveness required, since Electron/Chromium expose no ValuePattern|unit|done|
|INJ-019|Paste verification otherwise returns whether the focused element's text contains the first 20 characters of the injected text.|TextInjector.swift|wl-platform::inject|same|UIA TextPattern/ValuePattern read-back with the same prefix(20) rule|probe|done|
|INJ-020|Natural Mode speed presets map to characters per second: `slow` 2.5, `normal` (and any unrecognized value) 4.0, `expert` 6.5, giving base delays of 400 ms, 250 ms and about 153.8 ms.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-021|Per-character inter-key delay is `baseDelay` multiplied by a uniform random factor between 0.6 and 1.4 — slow 240-560 ms, normal 150-350 ms, expert 92.3-215.4 ms.|TextInjector.swift|wl-platform::inject|same|same; must not be batched into a single SendInput call|unit|done|
|INJ-022|Every synthesized key, including the unicode fallback, is held down for a uniform random 0.030 to 0.080 s so fast-key detectors register a real press.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-023|Natural Mode posts from a private-state event source so ambient Caps Lock and residual hotkey modifiers cannot corrupt characters.|TextInjector.swift|wl-platform::inject|same|No equivalent source; clear/restore modifier state explicitly or use KEYEVENTF_UNICODE|probe|done|
|INJ-024|Failure to create the private event source logs `Natural Mode: failed to create CGEventSource — falling back to paste` and falls back to the clipboard strategy; this is the only inter-strategy fallback in the app.|TextInjector.swift|wl-platform::inject|same|same trigger mapped to SendInput initialization failure|unit|done|
|INJ-025|Before typing, the layout map is (re)built on the MAIN THREAD and `Natural Mode typing N chars at C cps (layout map: M entries)` is logged. The `UCKeyTranslate` builder demands a `MainThreadMarker`, so an off-main call is a compile error rather than a runtime hazard.|TextInjector.swift|wl-platform::inject|MainThreadMarker required at the type level|GetKeyboardLayout + VkKeyScanEx; no main-thread requirement|unit|done|
|INJ-026|Newline is typed as a real Return key, virtual key 36, with empty flags.|TextInjector.swift|wl-platform::inject|same|VK_RETURN|probe|todo — Return key path not exercised; 'punctuation correct' does not cover \n|
|INJ-027|Tab is typed as a real Tab key, virtual key 48, with empty flags.|TextInjector.swift|wl-platform::inject|same|VK_TAB|probe|todo — Tab key path not exercised|
|INJ-028|Characters present in the layout map are typed with their mapped virtual key and flags, and the flags are always pinned on both down and up events even when empty, so ambient modifiers cannot ride along.|TextInjector.swift|wl-platform::inject|same|VkKeyScanEx shift state, explicitly pinned|probe|done|
|INJ-029|Characters absent from the layout map are typed via virtual key 0 down/up with empty flags and the character's UTF-16 units attached to both events.|TextInjector.swift|wl-platform::inject|same|SendInput KEYEVENTF_UNICODE; surrogate pairs sent as two input events|probe|todo — probe layer not yet run for this capability|
|INJ-030|Natural Mode calls back true unconditionally at the end — there is no verification in this strategy.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-031|The reverse layout map is keyed by the current input source id and rebuilt only when that id changes or the map is empty.|TextInjector.swift|wl-platform::inject|same|rebuilt on WM_INPUTLANGCHANGE|unit|done|
|INJ-032|The layout map is built by translating virtual keys 0..<128 across exactly four modifier combos — none, shift, option, shift+option — with Command deliberately excluded because it suppresses character generation.|TextInjector.swift|wl-platform::inject|same|VkKeyScanEx yields vk + shift state directly; AltGr handled as ctrl+alt|unit|done|
|INJ-033|A translation result is accepted only when it succeeded, produced at least one unit, is exactly one Character, and its first scalar is at least 0x20; the first matching combo wins, so unshifted mappings are preferred.|TextInjector.swift|wl-platform::inject|same|same acceptance rules applied to VkKeyScanEx results|unit|done|
|INJ-034|Unavailable keyboard layout data logs `Natural Mode: keyboard layout data unavailable` and leaves the previous or empty map in place, so every character goes through the unicode fallback.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-035|There is zero per-app special-casing in the injector: no hardcoded bundle ids, no per-app strategy overrides.|TextInjector.swift|wl-platform::inject|same|same|unit|done|
|INJ-036|`readSelectedText()` reads the system-wide focused element's selected-text attribute and returns nil when absent or empty; it exists but is unused by the polish flow, which synthesizes a copy instead.|TextInjector.swift|wl-platform::inject|same|UIA TextPattern GetSelection|probe|done|
|INJ-037|`readFocusedElementText()` reads the focused element's value attribute and returns a one-element array or an empty array.|TextInjector.swift|wl-platform::inject|same|UIA GetFocusedElement -> ValuePattern/TextPattern|probe|done|
|INJ-038|When the frontmost app classifies as `email` and `emailAutoSignature` is on, the injected text gains a suffix — `\n\n— Spoken with Wispr Lightning` when `emailSignatureOption == "spoken_with_lightning"`, otherwise `\n\n— Written with Wispr Lightning`.|AppInfoDetector.swift|wl-core::fsm|same|same|unit|done|
|INJ-039|Raw transcript injection is skipped entirely when `autoPolish && polishEnabled` — only the polished text is injected.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|INJ-040|Injection into an elevated window from an unelevated process fails; this must be detected and surfaced with guidance rather than silently doing nothing.|n/a (new)|wl-platform::windows::inject|n/a — TCC Accessibility grant is the analogue and is already prompted at launch|UIPI detection plus user-facing guidance (new requirement, not parity)|probe|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|INJ-041|In Natural Mode the target application's OWN text substitution applies to synthesized keystrokes — TextEdit converts a typed `'` into a typographic apostrophe. This is inherent to synthesizing real key events rather than a defect, and it matches the Swift original, which also typed key events. The Paste strategy is immune because it inserts text wholesale. Expected behavior: do NOT 'fix' it by disabling Natural Mode.|TextInjector.swift|wl-platform::inject|same|same (Windows apps with autocorrect behave equivalently)|probe|done|

## 4. Context capture (frontmost app, AX/UIA text, screen OCR)

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|CTX-001|`getFrontmostAppInfo()` always returns exactly four keys: `name`, `bundle_id`, `type`, `url`.|AppInfoDetector.swift|wl-platform::appinfo|same|same|unit|done|
|CTX-002|No frontmost application -> `{"name":"", "bundle_id":"", "type":"other", "url":""}`.|AppInfoDetector.swift|wl-platform::appinfo|same|same|unit|done|
|CTX-003|`name` is the app's localized name or empty string; `bundle_id` is the bundle identifier or empty string.|AppInfoDetector.swift|wl-platform::appinfo|same|name from process image; bundle_id becomes the exe basename|probe|done|
|CTX-004|`type` is `messaging` for bundle ids com.slack.Slack, com.tinyspeck.slackmacgap, net.whatsapp.WhatsApp, com.tdesktop.Telegram, org.whispersystems.signal-desktop, com.discordapp.Discord.|AppInfoDetector.swift|wl-platform::appinfo|same|Windows table keyed on exe basename (slack.exe, WhatsApp.exe, Telegram.exe, Signal.exe, Discord.exe)|unit|done|
|CTX-005|`type` is `email` for com.apple.mail, com.microsoft.Outlook, com.google.Gmail.|AppInfoDetector.swift|wl-platform::appinfo|same|Windows table entry olk.exe / OUTLOOK.EXE|unit|done|
|CTX-006|`type` is `ai` for com.openai.chat, com.anthropic.claudefordesktop, com.todesktop.230313mzl4w4u92 (Cursor), com.microsoft.VSCode.|AppInfoDetector.swift|wl-platform::appinfo|same|Windows table entry Code.exe, Cursor.exe, ChatGPT.exe, Claude.exe|unit|done|
|CTX-007|Any other bundle id classifies as `other`; the checks run in the order messaging, email, ai, other.|AppInfoDetector.swift|wl-platform::appinfo|same|same ordering|unit|done|
|CTX-008|`url` is ALWAYS the empty string — there is no browser-URL detection of any kind and the empty field is the behavior to port.|AppInfoDetector.swift|wl-platform::appinfo|same|same (UIA address-bar reading is available but deliberately not wired)|unit|done|
|CTX-009|Frontmost app info is captured at recording start, not at injection time, so switching apps mid-dictation does not change the reported app.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|CTX-010|When `useAccessibilityContext` is true (default), the focused element's text is read on a dedicated background queue at recording start and drained at stop.|AppDelegate.swift|wl-platform::inject|same|UIA GetFocusedElement on a dedicated STA thread|probe|done|
|CTX-011|When `useScreenContext` is true (default false), OCR runs on a dedicated queue at recording start in parallel with recording and is drained at stop.|AppDelegate.swift|wl-platform::screentext|same|same|probe|done|
|CTX-012|OCR step 1: enumerate on-screen windows excluding desktop elements; failure logs `ScreenCaptureContext: Failed to get window list` and returns an empty array.|ScreenCaptureContext.swift|wl-platform::screentext|same|EnumWindows / GetForegroundWindow; same empty-array contract|unit|done|
|CTX-013|OCR step 2: resolve the frontmost application's pid; nil logs `No frontmost application` and returns an empty array.|ScreenCaptureContext.swift|wl-platform::screentext|same|GetForegroundWindow -> GetWindowThreadProcessId|unit|done|
|CTX-014|OCR step 3: pick the FIRST window whose owner pid matches and whose layer is 0 (normal layer, excluding panels and menus); none found logs `No window found for frontmost app` and returns an empty array.|ScreenCaptureContext.swift|wl-platform::screentext|same|foreground HWND directly, filtered to a top-level non-tool window|unit|done|
|CTX-015|OCR step 4: capture only that one window at full window bounds ignoring framing; a nil image logs `Screen capture returned nil — likely missing Screen Recording permission` and returns an empty array.|ScreenCaptureContext.swift|wl-platform::screentext|same (legacy CGWindowListCreateImage, not ScreenCaptureKit)|PrintWindow/BitBlt; note GPU-composited windows can return black|probe|done|
|CTX-016|OCR runs with recognition level `fast` and language correction OFF, with no language list, no region of interest, no minimum text height and no timeout.|ScreenCaptureContext.swift|wl-platform::screentext|same|Windows.Media.Ocr OcrEngine; requires an installed language pack|probe|done|
|CTX-017|A thrown OCR error logs `Vision OCR failed: <desc>` and returns an empty array.|ScreenCaptureContext.swift|wl-platform::screentext|same|same|unit|done|
|CTX-018|OCR results take the top candidate string per observation in natural observation order, with no confidence filter, no dedup, no length filter and no sorting, breaking once 50 lines are collected.|ScreenCaptureContext.swift|wl-platform::screentext|same|same 50-line cap; Windows OCR line segmentation differs so the captured context will differ|probe|done|
|CTX-019|No screen-recording permission preflight or request is performed; denial manifests only as a nil image, an empty result and a log line.|ScreenCaptureContext.swift|wl-platform::permissions|same|no equivalent permission gate on Windows|probe|todo — ScreenRecording was Granted, so the denial path was never taken|
|CTX-020|A wall-clock timeout bounds the OCR pass so a slow capture cannot stall the dictation pipeline.|ScreenCaptureContext.swift|wl-platform::screentext|added (Swift has no timeout at all)|required: WinRT OCR on a 4K window can take seconds|unit|done|

## 5. Media control & sound cues

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|SND-001|Both pause and resume early-return unless `settings.muteMusic` is true (default false).|MusicController.swift|wl-platform::media|same|same|unit|done|
|SND-002|Only two players are considered, detected by presence in the running-applications list: `com.apple.Music` and `com.spotify.client`.|MusicController.swift|wl-platform::media|same|DEVIATION: SMTC session manager covers every player, a deliberate superset|probe|done|
|SND-003|Pause for Apple Music runs the verbatim script `tell application "Music" to if player state is playing then` / `pause` / `return "paused"` / `end if` and treats the returned string `paused` as proof that it paused something. `NSAppleScript` is MAIN-THREAD ONLY — Apple's Thread Safety Summary lists it as the sole entry under Main Thread Only Classes, and since the December 2025 XProtect update a script object first CREATED off the main thread hangs the process — so `run_script` requires a `MainThreadMarker` and cannot be called from a worker.|MusicController.swift|wl-platform::macos::media|main-thread-only, enforced at the type level|GlobalSystemMediaTransportControlsSession.TryPauseAsync when PlaybackStatus is Playing|probe|done|
|SND-004|Pause for Spotify runs the same script shape against `Spotify`, with the same `paused` return contract.|MusicController.swift|wl-platform::macos::media|same|covered by the same SMTC session loop|probe|done|
|SND-005|Resume issues `tell application "Music" to play` and `tell application "Spotify" to play`, only for the players whose pause flag is set.|MusicController.swift|wl-platform::macos::media|same|TryPlayAsync only for sessions this app paused|probe|done|
|SND-006|All scripting errors are swallowed with no logging and no user feedback.|MusicController.swift|wl-platform::media|same|same (SMTC failures swallowed)|unit|done|
|SND-007|`pauseMusic()` fans out one background task per running player and then blocks the calling thread until both complete, which is why callers invoke it from a background queue.|MusicController.swift|wl-platform::media|same|same join semantics on the async SMTC calls|unit|done|
|SND-008|Pause flags are stored under a lock and `resumeMusic()` reads and clears them in one critical section, so only players this app paused are resumed.|MusicController.swift|wl-platform::media|same|same|unit|done|
|SND-009|A very short recording can call resume before the fire-and-forget pause has stored its flag, leaving music paused forever; the port serializes pause completion against resume so this cannot happen.|MusicController.swift|wl-platform::media|DEVIATION: documented Swift race is fixed|DEVIATION: documented Swift race is fixed|unit|done|
|SND-010|Because flags are cleared on read, a second resume is a no-op and a failed or slow pause is never retried.|MusicController.swift|wl-platform::media|same|same|unit|done|
|SND-011|Sound pack name resolves to `settings.selectedSoundPack ?? "default"`.|SoundManager.swift|wl-core::settings|same|same|unit|done|
|SND-012|A sound file is looked up in `Sounds/<pack>`; if missing and the pack is not `default`, it retries in `Sounds/default`; otherwise it resolves to nothing.|SoundManager.swift|src-tauri::commands|same (bundle resource lookup)|Tauri resource resolver next to the exe|unit|done|
|SND-013|`availablePacks()` lists the subdirectories of the bundled `Sounds` folder sorted alphabetically, returning `["default"]` when the folder is missing or empty; today the result is `["default", "v1", "v2", "v3"]`.|SoundManager.swift|src-tauri::commands|same|same|unit|done|
|SND-014|The Swift app loads exactly three file names — `dictation-start.wav`, `dictation-stop.wav`, `paste.wav` — while `achievement.wav`, `Notification.wav` and `popo-lock.wav` ship unreferenced; the port loads only the two that are ever played and ships neither `paste.wav` nor the three unreferenced assets.|SoundManager.swift|src-tauri::commands|DEVIATION DV10|DEVIATION DV10|unit|done|
|SND-015|The sound players are constructed with a failure-tolerant initializer (silently nil on failure) and prepared for playback up front; the port constructs two (start, stop) where Swift constructs three.|SoundManager.swift|wl-platform::media|DEVIATION DV10|DEVIATION DV10|unit|done|
|SND-016|The pack is reloaded on every settings-changed event, and a sound-pack-preview event plays the START sound.|SoundManager.swift|src-tauri::orchestrator|same|same|e2e|done|
|SND-017|`playStart()` fires at `startRecordingSession()` immediately BEFORE the recorder starts, on polish hotkey press before the synthetic copy, and on sound-pack preview; it seeks to 0 and plays.|SoundManager.swift|wl-platform::media|same|same|e2e|done|
|SND-018|When the start player is nil, the system sound `Tink` is played instead.|SoundManager.swift|wl-platform::media|same (NSSound named Tink)|No Windows equivalent — a bundled fallback WAV is shipped|unit|done|
|SND-019|`playStop()` fires at `stopRecordingSession()` immediately after the recorder stops, and after a successful polish injection inside the +0.3 s clipboard-restore block; it seeks to 0 and plays.|SoundManager.swift|wl-platform::media|same|same|e2e|done|
|SND-020|When the stop player is nil, the system sound `Pop` is played instead.|SoundManager.swift|wl-platform::media|same (NSSound named Pop)|No Windows equivalent — a bundled fallback WAV is shipped|unit|done|
|SND-021|RETIRED as dead code: `playPaste()` has exactly one occurrence in the Swift tree — its own definition at SoundManager.swift:94 — and zero call sites, and `paste.wav` is loaded but never played with no system-sound fallback. Neither the method nor the asset is ported.|SoundManager.swift:94|wl-platform::media|DEVIATION DV10|DEVIATION DV10|n/a|n/a|
|SND-022|All sound playback is gated by `settings.enableSounds` (default true).|SoundManager.swift|wl-platform::media|same|same|unit|done|
|SND-023|No playback volume is ever set (effective volume 1.0), there is no volume setting, playback is fire-and-forget on the default output device, and restarting a still-playing sound just seeks it to 0 rather than layering.|SoundManager.swift|wl-platform::media|same|same|unit|done|

## 6. Transcription protocol (Wispr WSS)

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|WSS-001|The socket connects to `wss://api.wisprflow.ai/llm/ws` with the request header `Encoding: json`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-002|Maximum WebSocket message size is set explicitly to 10485760 bytes (10 MB).|TranscriptionClient.swift|wl-providers::wispr|same|same; tokio-tungstenite defaults must be overridden for both max message and max frame|contract|done|
|WSS-003|`prewarmConnection()` establishes TCP+TLS at hotkey press so the auth round trip is not paying handshake cost at stop.|TranscriptionClient.swift|wl-providers::wispr|same|same|e2e|done|
|WSS-004|Message 1 is a JSON object whose `type` is the literal `auth` and its byte content must be identical to the reference implementation modulo `session_id` and `transcript_entity_uuid`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-005|`access_token` is the raw access token, or the empty string when no token is available.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-006|Top-level `app` is the frontmost app's `type` lowercased, defaulting to `other`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-007|`context.app` is the four-key object `{name, bundle_id, type, url}` taken verbatim from the frontmost-app snapshot, with `url` always empty.|TranscriptionClient.swift|wl-providers::wispr|same|bundle_id carries the exe basename|fixture|done|
|WSS-008|`context.ax_context` carries the focused-element text array, empty when `useAccessibilityContext` is off or nothing was readable.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-009|`context.ocr_context` carries up to 50 OCR lines, empty when `useScreenContext` is off.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-010|`context.dictionary_context` is the vocabulary phrase array capped at 50 entries.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-011|`context.dictionary_replacements` is a flat `{phrase: replacement}` map with no limit.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-012|`context.dictionary_snippets` wraps each expansion in a single-element array, so the wire shape is `{"phrase": ["expansion"]}` and never `{"phrase": "expansion"}`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-013|`context.user_first_name` and `context.user_last_name` come from the session, empty strings when unknown.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-014|`context.textbox_contents` is always `{}`, `context.content_text` always `""`, `context.variable_names` and `context.file_names` always `[]`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-015|`personalization_style_settings` is `settings.personalizationStyles` when `styleDetectionEnabled` is true, otherwise `{}`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-016|`language` is the configured language array, defaulting to `["en"]`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-017|`metadata.session_id` is the per-process session UUID, regenerated on every launch.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-018|`metadata.environment` is the literal `PRODUCTION`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-019|`metadata.client_platform` is the literal `darwin` on BOTH platforms. It is a single `const` in `wl-providers/src/wispr.rs`, so the change is one line if evidence ever appears. Sending `darwin` from Windows is the only value KNOWN to be accepted, because it is the only value the shipping client has ever sent; any other string is a guess, and a guess is the one thing that can produce a hard rejection. Being wrong about `darwin` costs at most server-side analytics attribution, while being wrong about a guessed value costs the entire transcription path.|TranscriptionClient.swift|wl-providers::wispr|same|`darwin` — PORT_PLAN section 8 debt item 3; trigger is observing what the real Windows client sends, or a rejection in the wild|contract|done|
|WSS-020|`metadata.client_version` is the literal `1.4.549`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-021|`metadata.transcript_entity_uuid` is the transcript UUID that later becomes `transcripts.id` in SQLite.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-022|`pipeline` is `["transcribe", "format"]` when `settings.aiFormatting` is true and `["transcribe"]` otherwise.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-023|`job_selectors` is `["creator"]` when `settings.creatorMode` is true and `[]` otherwise.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-024|`cleanup_level` is the literal `light` in the reference auth frame.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-025|`command_mode` is `true`, `debug_mode` is `false`, `use_staging_baseten` is `false`, `hyperlink_on` is `false` in the reference auth frame.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-026|`prefix_is_written` is `!axContext.isEmpty` — true only when accessibility context produced at least one line.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-027|The server must reply with `status == "auth"`; anything else or a non-string reply cancels the socket with `.internalServerError` and fails the request with `TranscriptionError.authFailed`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-028|Audio is chunked client-side at 500 packets per `append` message, roughly 20 s of audio and about 800 KB encoded.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-029|Each `append` carries `audio_packets.packets` as an array of ascii85 strings, one per 640-sample packet.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-030|`audio_packets.volumes[i]` is `round(sqrt(sum(s^2)/640) / 32768 * 10000) / 10000` over the Int16 samples — a 4-decimal value in roughly [0, 0.3052], rounded half-away-from-zero.|TranscriptionClient.swift|wl-core::packetizer|same|same|fixture|done|
|WSS-031|`audio_packets.packet_duration` is the literal `0.04` (chunkDurationMs 40 divided by 1000).|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-032|`audio_packets.audio_encoding` is the literal string `wav` even though the payload is raw headerless 16-bit little-endian mono PCM at 16 kHz with no WAV header prepended.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-033|`audio_packets.byte_encoding` is the literal string `ascii85`.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-034|`position` is the packet index of the chunk's first packet (0, then 500, then 1000, and so on), never a byte offset.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-035|`final` is true only on the append whose end index reaches the total packet count.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-036|Chunks are sent strictly sequentially, each send completion triggering the next; any send error logs `Wispr Lightning: WS chunk send failed: %@` and fails with `connectionFailed`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-037|Each chunk emits the verbose line `WS sending chunk <offset>..<<end> of <total> (<n> bytes, final=<bool>)`.|TranscriptionClient.swift|wl-providers::wispr|same|same|unit|done|
|WSS-038|Message N+1 is exactly `{"type":"commit", "total_packets": <totalPackets>}` and is sent only after the final append's send completion.|TranscriptionClient.swift|wl-providers::wispr|same|same|fixture|done|
|WSS-039|A commit send error logs `Wispr Lightning: WS commit send failed: %@` and fails with `connectionFailed`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-040|Commit success logs `Wispr Lightning: Audio sent — %d packets in %d chunks, waiting for transcription...` where chunk count is `(total + 499) / 500`, then starts the receive loop.|TranscriptionClient.swift|wl-providers::wispr|same|same|unit|done|
|WSS-041|The response deadline is `max(15.0, packetCount * 0.04 * 0.5)` seconds — at least 15 s, otherwise half the recorded duration.|TranscriptionClient.swift|wl-providers::wispr|same|same|unit|done|
|WSS-042|On deadline expiry: log `Wispr Lightning: WebSocket response timed out after %.0fs`, cancel the socket with `.abnormalClosure` and no reason, and complete with `TranscriptionError.timeout`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-043|Completion is guarded by a lock-protected `completed` flag in both the transcription driver and the receive loop, so a result and a timeout cannot both complete the request.|TranscriptionClient.swift|wl-providers::wispr|same|same|unit|done|
|WSS-044|Only text frames are handled; a binary frame or a non-JSON string falls through WITHOUT re-arming the receive loop, so the connection hangs until the deadline fires.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-045|`status == "text"` sets `resultText = body.llm_text ?? body.asr_text ?? ""` and logs `Wispr Lightning: Got %@ transcript: %d chars` with `final` or `partial`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-046|A message with top-level `final == false` (or absent) re-arms the receive loop; partials are logged only and never surfaced to the user.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-047|On top-level `final == true`: `duration = packetCount * 0.04`, `numWords = resultText.split(separator: " ").count`, a `TranscriptResult` is built with the transcript UUID and the as-received `asr_text`/`llm_text` (so `formattedText` is nil when only ASR ran), and the socket is cancelled with `.normalClosure`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-048|An empty final text yields `TranscriptionError.emptyResult` rather than a successful empty transcript.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-049|`status == "error"` logs `Wispr Lightning: Server error: %@`, cancels with `.internalServerError`, and fails with `serverError(<error field, default "unknown">)`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-050|`status == "info"` logs `Wispr Lightning: Server info: %@` using the `message` field (default empty) and continues receiving.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-051|Any other `status` value is ignored and the receive loop continues.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-052|A receive failure logs `Wispr Lightning: WS receive failed: %@` and fails with `connectionFailed`.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-053|Verbose logging emits `WS received: <first 500 chars of the frame>`.|TranscriptionClient.swift|wl-providers::wispr|same|same|unit|done|
|WSS-054|`TranscriptionError.authFailed` -> isRetryable false, userMessage `Authentication failed — please sign in again` (U+2014 em dash).|TranscriptEntry.swift|wl-providers::trait|same|same|unit|done|
|WSS-055|`TranscriptionError.connectionFailed` -> isRetryable true, userMessage `Connection failed — check your network`.|TranscriptEntry.swift|wl-providers::trait|same|same|unit|done|
|WSS-056|`TranscriptionError.serverError(detail)` -> isRetryable true, userMessage `Server error: <detail>`.|TranscriptEntry.swift|wl-providers::trait|same|same|unit|done|
|WSS-057|`TranscriptionError.timeout` -> isRetryable true, userMessage `Request timed out — server did not respond`.|TranscriptEntry.swift|wl-providers::trait|same|same|unit|done|
|WSS-058|`TranscriptionError.emptyResult` -> isRetryable false, userMessage `No transcription returned`.|TranscriptEntry.swift|wl-providers::trait|same|same|unit|done|
|WSS-059|Error trigger inventory is exhaustive: empty packet list -> emptyResult; token refresh failure -> authFailed; auth reply not `auth` -> authFailed; socket creation, JSON serialization, any send error, receive failure or missing prepared audio -> connectionFailed; `status == "error"` -> serverError; deadline -> timeout; empty final text -> emptyResult.|TranscriptionClient.swift|wl-providers::wispr|same|same|contract|done|
|WSS-060|The prepared-audio cache is keyed on a content hash of the packet buffer, so a retry reuses the encoding and two different recordings with equal packet counts can never collide.|TranscriptionClient.swift|wl-providers::wispr|DEVIATION DV8 (Swift keys the cache on packet count alone)|DEVIATION DV8|unit|done|
|WSS-061|Encoding runs on a dedicated user-initiated queue in parallel with the auth round trip and the two are joined before the first append is sent.|TranscriptionClient.swift|wl-providers::wispr|same|same|unit|done|
|WSS-062|Ascii85 iterates the input in 4-byte big-endian groups, zero-padding a short final group on the right, with the output buffer reserved as `(byteCount / 4 + 1) * 5`.|TranscriptionClient.swift|wl-core::ascii85|same|same|fixture|done|
|WSS-063|A FULL 4-byte group whose value is zero emits the single byte `z` (0x7A).|TranscriptionClient.swift|wl-core::ascii85|same|same|fixture|done|
|WSS-064|A SHORT all-zero tail group takes the normal path and produces a run of `!` characters, never `z`, matching CPython's a85encode padding rule.|TranscriptionClient.swift|wl-core::ascii85|same|same|fixture|done|
|WSS-065|Five digits are computed by repeated modulo/divide by 85, least-significant first into slot 4 working back to slot 0, each offset by +33 so the charset is the contiguous range `!` (0x21) through `u` (0x75).|TranscriptionClient.swift|wl-core::ascii85|same|same|fixture|done|
|WSS-066|A partial tail group of n bytes emits exactly n+1 characters from slot 0 forward, truncating the low-order digits.|TranscriptionClient.swift|wl-core::ascii85|same|same|fixture|done|
|WSS-067|There are no Adobe `<~`/`~>` delimiters, no line wrapping, no whitespace and no `y` space-fold in the output.|TranscriptionClient.swift|wl-core::ascii85|same|same|fixture|done|
|WSS-068|Each 1280-byte packet produces at most 1600 ascii85 characters (320 full groups), fewer when groups are all-zero, so digital silence compresses heavily.|TranscriptionClient.swift|wl-core::ascii85|same|same|fixture|done|

## 7. Provider abstraction & Deepgram (NEW functionality)

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|PRV-001|`TranscriptionProvider` is a `Send + Sync` trait exposing `id()`, `capabilities()`, `prewarm()`, `health()` and `transcribe()`; both Wispr and Deepgram implement it and nothing else in the app talks to a transcription backend directly.|n/a (new)|wl-providers::trait|same|same|contract|done|
|PRV-002|`ProviderId` has exactly two variants today, `Wispr` and `Deepgram`, and the selected provider is surfaced in history and logs.|n/a (new)|wl-providers::trait|same|same|unit|done|
|PRV-003|`prewarm()` on the Wispr provider opens the WSS TCP+TLS connection; on the Deepgram provider it warms the HTTPS connection pool.|n/a (new)|wl-providers::trait|same|same|contract|done|
|PRV-004|`health()` performs a credential check used by the settings UI and returns `Ok(())` or a `ProviderError`, without consuming audio.|n/a (new)|wl-providers::trait|same|same|contract|done|
|PRV-005|`transcribe(&TranscribeRequest)` is the single entry point and returns `TranscriptResult` or `ProviderError`; the request carries the packets, app context, screen context, dictionary data and language list.|n/a (new)|wl-providers::trait|same|same|contract|done|
|PRV-006|`ProviderCapabilities.server_side_formatting` is true for Wispr and false for Deepgram.|n/a (new)|wl-providers::trait|same|same|unit|done|
|PRV-007|`ProviderCapabilities.accepts_app_context` gates whether frontmost-app info is sent; Deepgram ignores it and the orchestrator must not fabricate a field for it.|n/a (new)|wl-providers::trait|same|same|unit|done|
|PRV-008|`ProviderCapabilities.accepts_screen_context` gates whether OCR lines are sent, so screen capture can be skipped entirely for a provider that cannot use it.|n/a (new)|wl-providers::trait|same|same|unit|done|
|PRV-009|`ProviderCapabilities.vocabulary` is `Full` for Wispr and `Keyterm { max_tokens: 500 }` for Deepgram; `None` disables all vocabulary transmission.|n/a (new)|wl-providers::trait|same|same|unit|done|
|PRV-010|`ProviderCapabilities.command_mode` reports whether the backend interprets spoken commands; Deepgram reports false, so the Voice Commands setting has no effect under it.|n/a (new)|wl-providers::trait|same|same|unit|done|
|PRV-011|When `!server_side_formatting`, `wl-core` runs a post-processor over the raw ASR text; when it is true the transcript is used verbatim.|n/a (new)|wl-core::postproc|same|same|unit|done|
|PRV-012|Post-processor step 1 applies dictionary replacements client-side on word boundaries, preserving the original casing of the matched token.|n/a (new)|wl-core::postproc|same|same|unit|done|
|PRV-013|Post-processor step 2 expands dictionary snippets, using the same phrase-to-expansion mapping that Wispr receives as `dictionary_snippets`.|n/a (new)|wl-core::postproc|same|same|unit|done|
|PRV-014|Post-processor step 3 runs the existing polish pass when `settings.aiFormatting` is on, giving Deepgram output formatting comparable to Wispr's server-side pass.|n/a (new)|wl-core::postproc|same|same|contract|done|
|PRV-015|Deepgram transcription is a single batch `POST /v1/listen` carrying the whole buffer captured at key release; no streaming session is opened.|n/a (new)|wl-providers::deepgram|DECISION D4|DECISION D4|contract|done|
|PRV-016|Streaming `endpointing` and `utterance_end_ms` are deliberately not used, because the key release is already a perfect turn signal and streaming would add at least 1000 ms of latency.|n/a (new)|wl-providers::deepgram|DECISION D4|DECISION D4|contract|done|
|PRV-017|`keyterm` is populated with the top 50 vocabulary phrases ordered by `frequency_used` descending, matching the cap already applied to `dictionary_context`.|n/a (new)|wl-providers::deepgram|same|same|unit|done|
|PRV-018|Each keyterm is validated before transmission and rejected if it contains `,` or `;` or matches the `:<number>` boost suffix form.|n/a (new)|wl-providers::deepgram|same|same|unit|done|
|PRV-019|Malformed keyterms are known to return HTTP 200 while silently boosting nothing, so validation failures are reported locally rather than assumed successful from the status code.|n/a (new)|wl-providers::deepgram|same|same|contract|done|
|PRV-020|Query-string encoding for the Deepgram request is asserted exactly (repeated `keyterm` parameters, percent-encoding of spaces and punctuation).|n/a (new)|wl-providers::deepgram|same|same|contract|done|
|PRV-021|Provider selection is GLOBAL: the single key `Settings::provider` is the source of truth, persisted alongside every other settings key. There is no per-profile provider concept and none is invented.|n/a (new)|wl-core::settings|same|same|unit|done|
|PRV-022|`ProviderError` preserves the `is_retryable` classification of `TranscriptionError`, so the retry loop, the retry overlay and the Save affordance behave identically regardless of provider.|n/a (new)|wl-providers::trait|same|same|unit|done|
|PRV-023|The Deepgram API key is stored in the OS keyring, never in `settings.json`.|n/a (new)|wl-providers::auth|DECISION D8 macOS Keychain|DECISION D8 Windows Credential Manager|probe|todo — probe layer not yet run for this capability|
|PRV-024|One contract suite runs against every provider over a local mock server and covers success, empty result, auth failure, server error, timeout, retryability classification and cancellation.|n/a (new)|wl-providers::trait|same|same|contract|done|
|PRV-025|The settings UI states plainly, per provider, which capabilities are and are not available (server-side formatting, app context, screen context, vocabulary mode, command mode).|n/a (new)|ui/settings|same|same|e2e|todo — no e2e run exercises this path yet|
|PRV-026|Deepgram auto-detect sends `detect_language=true` and OMITS the `language` parameter entirely; the literal string `auto` is never passed through to the API. Detection spans 35 languages and is weakest on short utterances, which is most of push-to-talk dictation.|n/a (new)|wl-providers::deepgram|same|same|contract|done|
|PRV-027|Deepgram language selection TRANSLATES at the boundary in `wl-providers/src/deepgram.rs` and never passes our codes through: no languages selected -> request English; exactly one selected -> that code translated to Deepgram's tag; two or more -> `multi`, the code-switching mode spanning English, Spanish, French, German, Hindi, Russian, Portuguese, Japanese, Italian and Dutch, where a selected language outside that set will not be recognised. `Settings::languages` holds Wispr Flow's private vocabulary (`engb`, `zhcn`, `dech`, `hien`, `auto` are not BCP-47), so pass-through is never correct.|n/a (new)|wl-providers::deepgram|same|same|contract|done|
|PRV-028|When the detected or requested language is not available on the selected Deepgram model, Deepgram SILENTLY falls back down the chain Nova-3 -> Nova-2 -> Nova-1 -> Enhanced -> Base; the response carries no error and no signal that the model changed. The batch response cannot reveal it either: the documented pre-recorded `metadata` object is `{request_id, transaction_key, sha256, created, duration, channels, language?}` with no `models` and no `model_info` — that field exists only on STREAMING's slimmer `{request_id, model_info, model_uuid}`. The only documented per-request language signal in batch is `detected_language` on the channel result.|n/a (new)|wl-providers::deepgram|same|same|contract|done|
|PRV-029|When `deepgram_keyterm_boost` is on AND `deepgramModel` is Nova-3 family AND the language mode is Detect, the provider logs a warning that keyterm boosting may not apply, because auto-detect can drop the request to a non-Nova-3 model on which boosting does nothing; the settings pane states the same in one sentence appended to the auto-detect notice (SET-110). The Nova-3-family clause is REQUIRED: warning a user on nova-2 that auto-detect may drop them off Nova-3 is incoherent, since they were never on it. Fully decidable at REQUEST time, so no response parsing and no dependency on an undocumented field. A runtime detection mechanism is deliberately NOT built.|n/a (new)|wl-providers::deepgram|same|same|unit|done|
|PRV-030|When `deepgram_keyterm_boost` is on AND `deepgramModel` is NOT Nova-3 family, the provider logs `keyterm boosting is ignored: <model> is not a Nova-3 family model.` — unconditional and unrelated to auto-detect. This case is reachable because SET-096 greys the switch without writing `false`. It is mutually exclusive with PRV-029 by construction, so no configuration produces both warnings and every configuration in which boost silently does nothing produces exactly one log line. Deliberately NOT surfaced in the UI: the greyed switch already communicates it on screen, but the log still needs it for a support report.|n/a (new)|wl-providers::deepgram|same|same|unit|done|
|PRV-031|The `zh` collision, which is why translation is mandatory rather than tidy: our picker's `zh` means Traditional Chinese, Deepgram's `zh` means Simplified. Passing it through returns HTTP 200 and a confident, clean transcript IN THE WRONG SCRIPT — no status code, no `Test connection`, and no schema catches it. Where a provider's parameter vocabulary overlaps ours but disagrees on meaning, an accepted-and-wrong value is strictly more dangerous than a rejected one. Relatedly, the roughly 40 codes Nova-3 does not support are deliberately left untranslated and allowed to FAIL VISIBLY, because a failure the user can act on beats a silent substitution.|n/a (new)|wl-providers::deepgram|same|same|contract|done|

## 8. Polish (manual hotkey flow + auto-polish)

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|POL-001|Polish posts to `https://api.wisprflow.ai/llm/polish_text`.|PolishService.swift|wl-providers::polish|same|same|contract|done|
|POL-002|Empty input text fails immediately with `emptyResult` and user message `No transcription returned`; no request is made.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-003|An invalid session triggers a token refresh first; refresh failure (or a deallocated service) fails with `authFailed`, refresh success proceeds.|PolishService.swift|wl-providers::polish|same|same|contract|done|
|POL-004|A malformed URL or a JSON serialization failure fails with `connectionFailed`.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-005|Request header `Content-Type: application/json`.|PolishService.swift|wl-providers::polish|same|same|fixture|done|
|POL-006|Request header `Authorization` carries the RAW access token with no `Bearer ` prefix, and the empty string when the token is nil.|PolishService.swift|wl-providers::polish|same|same (HTTP helpers must not add the prefix)|fixture|done|
|POL-007|Request header `Cache-Control: no-cache, no-store, must-revalidate`.|PolishService.swift|wl-providers::polish|same|same|fixture|done|
|POL-008|Request body has exactly five keys: `selected_text`, `instructions`, `provider_config`, `writing_samples`, `custom_prompt`.|PolishService.swift|wl-providers::polish|same|same|fixture|done|
|POL-009|`instructions` maps each ENABLED instruction label to `true`; disabled instructions are omitted entirely and never sent as `false`.|PolishService.swift|wl-providers::polish|same|same|fixture|done|
|POL-010|`provider_config`, `writing_samples` and `custom_prompt` are always JSON null — the client never sends prompt text or a model name.|PolishService.swift|wl-providers::polish|same|same|fixture|done|
|POL-011|With verbose logging on, `Polish request body: <json>` is logged before the request goes out.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-012|No explicit request timeout is set, so the effective request timeout is 60 s; the port sets 60 s explicitly because the HTTP client has no default.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-013|A transport error logs `Wispr Lightning: Polish request failed: %@` and fails with `connectionFailed`.|PolishService.swift|wl-providers::polish|same|same|contract|done|
|POL-014|The HTTP status is classified BEFORE any body parsing: 401/403 -> `authFailed`, 402 -> `quotaExceeded`, 429 -> `rateLimited`, 408/504 -> `timeout`. The Swift original never inspects the status, so a 401 surfaces as `Server error: 401`, is classified retryable, and burns two automatic retries against an endpoint that will keep refusing — the user is never told the one thing they can act on, which is to sign in again.|PolishService.swift|wl-providers::polish|DEVIATION DV11|DEVIATION DV11|contract|done|
|POL-015|With verbose logging on, `Polish response: <raw body>` is logged.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-016|`polished_text` is parsed ONLY on a 2xx response; a non-empty string is the success path, and only `polished_text` and `status` are ever read from the body.|PolishService.swift|wl-providers::polish|DEVIATION DV11 (Swift parses the body regardless of status)|DEVIATION DV11|contract|done|
|POL-017|On a 2xx whose body has no usable `polished_text`, `status` (default `unknown`) is logged as `Wispr Lightning: Polish failed with status: %@` and returned as `serverError(status)` with user message `Server error: <status>`; a non-2xx never reaches this path.|PolishService.swift|wl-providers::polish|DEVIATION DV11|DEVIATION DV11|contract|done|
|POL-018|`PolishResult.id` is a UUID generated BEFORE the request is sent, and it becomes `polish.id` in SQLite.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-019|`initialWordCount` and `polishedWordCount` are counted with a literal-space split that omits empty subsequences, not general whitespace splitting.|PolishService.swift|wl-core::postproc|same|same|unit|done|
|POL-020|`processingTime` is wall-clock seconds across the entire request as a Double.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-021|`instruction` is the active instruction labels joined with `. ` (period + space) with no trailing period, in unordered map-iteration order.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-022|The completion handler runs on the HTTP client's delegate queue, not the main queue; callers hop to main themselves.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|POL-023|Manual polish aborts unless `settings.polishEnabled` is true.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|POL-024|Manual polish with no enabled instructions logs `Polish: no instructions enabled` and aborts before touching the clipboard.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|POL-025|Manual polish captures frontmost app info, plays the START sound, then shows the overlay and immediately puts it into Processing.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|POL-026|Manual polish saves the clipboard off-main and synthesizes Cmd+C as virtual key 8 (`C`) with `.maskCommand` posted to the HID event tap.|AppDelegate.swift|wl-platform::inject|same|SendInput VK_CONTROL + 0x43|probe|todo — probe layer not yet run for this capability|
|POL-027|Manual polish then sleeps a fixed 150 ms waiting for the target app to fill the clipboard.|AppDelegate.swift|wl-platform::clipboard|same|same fixed 150 ms deadline; clipboard sequence-number polling may bound it earlier|unit|done|
|POL-028|The copied text is read as a plain string on the main queue synchronously.|AppDelegate.swift|wl-platform::clipboard|same|same|unit|done|
|POL-029|Empty or nil copied text restores the clipboard and shows overlay error `Select text to polish`, then aborts.|AppDelegate.swift|wl-core::fsm|same|same|e2e|done|
|POL-030|On success the polished text is injected, then 0.3 s later the original clipboard is restored, the STOP sound plays and the overlay hides — note this restore is 300 ms after the injector's own 250 ms restore.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|POL-031|A successful manual polish is persisted with `app` set to the frontmost app's name.|AppDelegate.swift|wl-core::stores::polish|same|same|unit|done|
|POL-032|On failure the clipboard is restored and the overlay shows the error's `userMessage`.|AppDelegate.swift|wl-core::fsm|same|same|e2e|done|
|POL-033|Auto-polish runs only when `settings.autoPolish && settings.polishEnabled && !activePolishInstructions.isEmpty`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|POL-034|During auto-polish the raw transcript injection is skipped and the overlay remains in Processing until the polish call resolves.|AppDelegate.swift|wl-core::fsm|same|same|e2e|done|
|POL-035|Auto-polish success injects the polished text, hides the overlay, and persists the result with `app` set to the empty string.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|POL-036|Auto-polish failure logs the error and injects the ORIGINAL transcript as a fallback, then hides the overlay.|AppDelegate.swift|wl-core::fsm|same|same|e2e|done|
|POL-037|Missing data or a non-object top-level JSON body on an otherwise 2xx response logs `Wispr Lightning: Polish response parse failed` and fails with `connectionFailed`.|PolishService.swift|wl-providers::polish|same|same|contract|done|
|POL-038|`quotaExceeded` is a NEW error case with no Swift counterpart, raised for HTTP 402 ONLY, with `is_retryable = false` and user message `Out of credits — check your <provider> account`. 402 never clears by retrying, so retrying it is the exact defect DV11 exists to fix.|PolishService.swift|wl-providers::polish|DEVIATION DV11|DEVIATION DV11|contract|done|
|POL-039|`rateLimited` is a NEW error case with no Swift counterpart, raised for HTTP 429 ONLY, with `is_retryable = true` and user message `Rate limited — try again in a moment`. 429 always clears with backoff, so refusing to retry it would strand a recording that would have succeeded. It must NOT share a variant with 402.|PolishService.swift|wl-providers::polish|DEVIATION DV11|DEVIATION DV11|contract|done|

## 9. Auth & session

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|AUT-001|Sign-in opens the system browser at `https://dodjkfqhwrzqjwkfnthl.supabase.co/auth/v1/authorize?provider=google&redirect_to=wispr-flow://auth/google/success` on BOTH platforms. `redirect_to` must match Supabase's server-side allow-list, which belongs to Wispr Flow's project and which we cannot edit, so this is the only usable value anywhere — a loopback redirect is not available.|AuthService.swift|wl-providers::auth|same|same URL; the scheme is registered on Windows only if nobody already owns it|e2e|todo — no e2e run exercises this path yet|
|AUT-002|The redirect URI is percent-encoded with a URL-query allowed set that leaves `:` and `/` unescaped, so `wispr-flow://auth/google/success` appears literally in the authorize URL.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-003|The authorize URL is handed to the OS default browser, not an embedded webview.|AuthService.swift|wl-providers::auth|same|same|e2e|todo — no e2e run exercises this path yet|
|AUT-004|The app registers URL schemes `wispr-flow` and `wisprlightning`; `wispr-flow` is deliberately shared with the commercial Wispr Flow app and on macOS whichever app is foregrounded wins. OBSERVED IN THE WILD, not hypothetical: on the development machine the Swift app this port replaces is installed at `/Applications/Wispr Lightning.app`, registers BOTH schemes, and has been running for over five days, so a deep link may be delivered to it instead of to the port. A delivery that reaches the wrong app is exactly this hazard — recording it is a finding, and any deep-link row that cannot PROVE delivery reached the port must stay open rather than be marked closed.|Info.plist|src-tauri::commands|two installed apps share the identifier and both schemes; LaunchServices arbitration decides|register `wispr-flow` ONLY if nobody already owns it — HKCU is last-writer-wins with no arbitration, so hijacking Wispr Flow's scheme is unacceptable|manual: install alongside Wispr Flow on macOS and confirm the deep link still reaches one of the two apps|todo — manual step not yet performed|
|AUT-005|The OAuth callback arrives through the OS deep-link handler for the registered scheme.|AppDelegate.swift|src-tauri::commands|same|protocol handler delivers the URL to the running instance; if the scheme is already owned, the `Paste sign-in link` fallback is the path instead|e2e|todo — no e2e run exercises this path yet|
|AUT-006|Only callback URLs whose string contains `auth/` are handled; every other deep link is ignored.|AppDelegate.swift|wl-providers::auth|same|same|unit|done|
|AUT-007|Receipt logs `Wispr Lightning: Received URL callback: %@`.|AppDelegate.swift|wl-providers::auth|same|same|unit|done|
|AUT-008|Callback query items are parsed into a map where the LAST duplicate key wins.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-009|When `access_token` is absent from the query, the URL fragment is split on `&`, each pair split on `=` with at most one split, and values are percent-decoded.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-010|Both `access_token` and `refresh_token` are required; otherwise the callback completes false.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-011|`expiresAt` is `Double(params["expires_at"] ?? "0") ?? 0`.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-012|The JWT payload is decoded WITHOUT signature verification: split on `.`, require at least two segments, translate base64url to base64 (`-`->`+`, `_`->`/`), pad with `=` until the length is a multiple of 4, then parse as JSON.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-013|From the JWT payload: `sub` -> userId, `email` -> userEmail, `user_metadata.avatar_url` or `user_metadata.picture` -> avatarURL.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-014|`user_metadata.full_name` or `user_metadata.name` (default empty) is split on a space with at most one split into first and last name.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-015|If first or last name is still nil after the JWT pass, the query params `first_name` and `last_name` are used.|AuthService.swift|wl-providers::auth|same|same|unit|done|
|AUT-016|A successful callback saves the session, logs `Wispr Lightning: Sign in successful` and posts the session-changed event on the main queue; a failed one logs `Wispr Lightning: Sign in failed`.|AppDelegate.swift|wl-providers::auth|same|same|e2e|todo — no e2e run exercises this path yet|
|AUT-017|`Session.sessionId` is a UUID regenerated on every process launch and is sent as `metadata.session_id` on every transcription.|Session.swift|wl-providers::auth|same|same|unit|done|
|AUT-018|`isValid` is `accessToken != nil` AND (`expiresAt == 0` OR `now <= expiresAt - 60`) — a 60-second clock-skew margin.|Session.swift|wl-providers::auth|same|same|unit|done|
|AUT-019|`load()` prefers the app's own credential store and, on a miss, migrates Lightning's OWN legacy plaintext file at `~/Library/Application Support/WisprLightning/session.json` (the spec's `liteSessionURL`) into it.|Session.swift|wl-providers::auth|same|n/a — Windows never had a Swift build, so there is no plaintext session to migrate|unit|done|
|AUT-039|On a miss in BOTH the credential store and Lightning's own legacy file, `load()` falls back to the commercial app's `~/Library/Application Support/Wispr Flow/session.json`, saves a copy into Lightning's own store, and logs `Wispr Lightning: Migrated session from Wispr Flow (%@)`.|Session.swift|wl-providers::auth|not implemented: `wl_core::paths::wispr_flow_session_file()` has no callers in the workspace and the log string occurs in no .rs file|n/a — the Windows Wispr Flow storage format is unverified and shipping a guess is worse than not shipping it|unit|todo — startup fallback still absent: load_tokens() reads only app_support_dir()/session.json (session.rs:412). wispr_flow_session_file() now HAS a caller (flow_watcher.rs:76) but that is the watcher-adoption path, not load()'s startup fallback; the log line occurs in no .rs file|
|AUT-020|Session parsing finds the FIRST key whose name contains the substring `auth-token`; its value may be either a JSON string to re-parse or a nested object.|Session.swift|wl-providers::auth|same|same read-compat retained|fixture|done|
|AUT-021|Parsing reads `access_token`, `refresh_token`, `expires_at`, `user.id`, `user.email`, `user.user_metadata.avatar_url` or `picture`, and `user.user_metadata.full_name` or `name`; a nil `access_token` fails the parse.|Session.swift|wl-providers::auth|same|same|fixture|done|
|AUT-022|When avatarURL or userEmail is nil after parsing, the JWT is decoded to fill `email`, metadata, and `exp` — which becomes the authoritative `expiresAt` when it is in the future.|Session.swift|wl-providers::auth|same|same|unit|done|
|AUT-023|Token refresh posts to `https://dodjkfqhwrzqjwkfnthl.supabase.co/auth/v1/token?grant_type=refresh_token` with `Content-Type: application/json`, `apikey: <supabase anon key>`, `Authorization: Bearer <supabase anon key>` and body `{"refresh_token": "<token>"}`.|Session.swift|wl-providers::auth|same|same|contract|done|
|AUT-024|Refresh requires `access_token` and `refresh_token` in the response; `expiresAt` is `expires_at` when in the future, else `now + expires_in`, else 0, and may then be overridden by the JWT `exp`.|Session.swift|wl-providers::auth|same|same|contract|done|
|AUT-025|Refresh success saves the session, verbose-logs the first 300 characters of the response, logs `Wispr Lightning: Token refreshed successfully` and completes true.|Session.swift|wl-providers::auth|same|same|contract|done|
|AUT-026|Any refresh failure logs `Wispr Lightning: Token refresh failed: %@` and completes false; there is no retry and no timeout override, so the effective timeout is 60 s.|Session.swift|wl-providers::auth|same|same (explicit 60 s timeout required)|contract|done|
|AUT-027|Access and refresh tokens are stored in the OS keyring; the plaintext `session.json` is still READ for migration but never written as the primary store.|Session.swift|wl-providers::auth|DECISION D8 / DEVIATION from plaintext session.json|DECISION D8 / DEVIATION from plaintext session.json|probe|todo — probe layer not yet run for this capability|
|AUT-028|The legacy `session.json` write format is a single key `sb-dodjkfqhwrzqjwkfnthl-auth-token` whose value is the inner object serialized as a JSON STRING, pretty-printed.|Session.swift|wl-providers::auth|read-compat retained for migration|read-compat retained for migration|fixture|done|
|AUT-029|In that legacy format `full_name` is always `"<first> <last>"` — a bare space when both are nil — and `avatar_url` is omitted entirely when nil.|Session.swift|wl-providers::auth|read-compat retained|read-compat retained|fixture|done|
|AUT-030|`clear()` nils the token and identity fields, sets `expiresAt = 0`, deletes the session file and posts session-changed — but does NOT clear `avatarURL`.|Session.swift|wl-providers::auth|same|same|unit|done|
|AUT-031|A directory watcher on `~/Library/Application Support/Wispr Flow/` reacts to write and rename events; the directory is created first if missing, events are debounced by 150 ms, and the watcher is torn down at exit.|AppDelegate.swift|src-tauri::flow_watcher|notify 8 FSEvents directory watch, NonRecursive, 150 ms debounce; started from setup(), stopped at RunEvent::Exit|n/a — macOS-gated per PORT_PLAN section 4|probe|done|
|AUT-032|On a watcher event the session is adopted only when the current one is NOT already valid (`flow_watcher.rs:181` checks `is_valid()` first, so a redundant wake costs one check); otherwise the foreign blob is passed to `Session::adopt` (`flow_watcher.rs:195`), which refuses a blob with no token, and a successful adoption publishes `session:changed`.|AppDelegate.swift|src-tauri::flow_watcher|7 tests over a temp directory that drive the REAL notify watcher (FlowWatcher::watch plus fs::write), not just the adopt path|n/a|unit|done|
|AUT-033|Token refresh classifies by HTTP status BEFORE deciding the outcome: 400/401/403 means the refresh token is genuinely dead -> `authFailed`, and only this path asks the user to sign in again.|Session.swift|wl-providers::auth|DEVIATION DV11|DEVIATION DV11|contract|done|
|AUT-034|Token refresh on 408, any 5xx, a timeout or a transport error yields `timeout` or `connectionFailed` — both retryable — so the pipeline's existing retry path handles it and the user NEVER sees a sign-in prompt for a server hiccup.|Session.swift|wl-providers::auth|DEVIATION DV11|DEVIATION DV11|contract|done|
|AUT-035|The Swift defect being fixed: every refresh failure collapses to `authFailed`, so a transient Supabase 500 or a flaky network throws the user at a sign-in screen and DISCARDS a recording that a retry would have completed.|Session.swift|wl-providers::auth|DEVIATION DV11|DEVIATION DV11|unit|done|
|AUT-036|When the `wispr-flow` scheme is already owned by another application, the app offers an explicit `Paste sign-in link` fallback so the user can complete OAuth by pasting the callback URL manually; the pasted URL goes through the same `auth/` filter and parser as a delivered deep link, and `auth_submit_callback` is the single owner of the trimming rule. Its rejection message is the canonical string that the client-side validator (SET-117) reproduces verbatim, so one failure class yields one sentence.|n/a (new)|wl-providers::auth|n/a — macOS arbitrates by foreground app, so the scheme always reaches one of the two|required whenever the scheme is taken|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|AUT-037|Windows sign-in is closed in ALL THREE registry states, with no configuration left where a user is stuck: (1) nobody owns `wispr-flow://` — `register_runtime_schemes` registers it, the deep link works, `auth_needs_manual_callback()` is false and the paste field stays hidden; (2) the app owns it — same behavior; (3) another application owns it — the app leaves the key strictly alone, the command returns true, and the paste field appears and completes sign-in through `auth_submit_callback`. This is a JOINT claim: neither half proves it, because state 3 is the only one where the UI acts and the UI cannot detect state 3 on its own. A backend-only row registers the scheme correctly and still leaves the user stuck; a UI-only row shows an escape hatch nobody can reach. Backend half lives in `src-tauri/src/deeplink.rs`.|n/a (new)|src-tauri::deeplink + ui/settings|n/a — macOS arbitrates by foreground app, so all three states collapse to a working deep link|all three states verified|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|AUT-038|`scheme_owner` in `src-tauri/src/deeplink.rs` reads `HKEY_CLASSES_ROOT\wispr-flow\shell\open\command` DIRECTLY rather than using the deep-link plugin's `DeepLink::is_registered`, because that helper returns false BOTH when nobody owns the scheme and when another app does — and those two states require OPPOSITE behavior (register it, versus leave it strictly alone and reveal the paste field). Distinguishing them is what makes states 1 and 3 separable at all. The reason is also a doc comment on `scheme_owner`, but a row naming it survives better than a comment.|n/a (new)|src-tauri::deeplink|n/a|required|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|

## 10. Persistence (SQLite schema, every store query, settings JSON)

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|DB-001|At startup the app-data directory `~/Library/Application Support/WisprLightning` is created with intermediate directories, and any error is swallowed.|DatabaseManager.swift|wl-core::db|same|`%APPDATA%\WisprLightning` via the Tauri app-data dir|unit|done|
|DB-002|If `history.db` exists and `lightning.db` does not, the file is MOVED to `lightning.db` and `Wispr Lightning: Migrated history.db → lightning.db` (real U+2192 arrow) is logged.|DatabaseManager.swift|wl-core::db|same|n/a — no Windows install ever had history.db|unit|done|
|DB-003|If `lightning.db` already exists, `history.db` is left orphaned on disk and never deleted.|DatabaseManager.swift|wl-core::db|same|n/a|unit|done|
|DB-004|On a successful open, `PRAGMA journal_mode=WAL;` runs and `Wispr Lightning: Database opened at %@` is logged.|DatabaseManager.swift|wl-core::db|same|same|unit|done|
|DB-005|On a failed open the handle is nil, `Wispr Lightning: Failed to open database at %@` is logged, and EVERY store silently no-ops returning empty results rather than erroring.|DatabaseManager.swift|wl-core::db|same|same|unit|done|
|DB-006|The raw exec helper ignores the SQLite return code entirely — a failed `CREATE TABLE` is invisible.|DatabaseManager.swift|wl-core::db|same|same|unit|done|
|DB-007|The transaction helper issues `BEGIN TRANSACTION;`, runs the block, then `COMMIT;` — there is NO rollback path.|DatabaseManager.swift|wl-core::db|same|same|unit|done|
|DB-008|Text column reads return nil for SQL NULL, and optional text binds either the value or SQL NULL.|DatabaseManager.swift|wl-core::db|same|same|unit|done|
|DB-009|The database is closed from the terminate handler, after a history-store close that is itself a no-op.|DatabaseManager.swift|wl-core::db|same|same|unit|done|
|DB-010|There is no `user_version`, no `ALTER TABLE` and no schema versioning anywhere; schema evolution is only `CREATE TABLE IF NOT EXISTS` from each store's init, so an older DB missing a newer column is never upgraded.|DatabaseManager.swift|wl-core::db|same|same|fixture|done|
|DB-011|`transcripts` is created as `CREATE TABLE IF NOT EXISTS transcripts (id TEXT PRIMARY KEY, asr_text TEXT, formatted_text TEXT, timestamp REAL, app_name TEXT, app_bundle_id TEXT, duration REAL, num_words INTEGER, language TEXT);`|HistoryStore.swift|wl-core::stores::history|same|same|fixture|done|
|DB-012|`dictionary` is created with columns id TEXT PRIMARY KEY, phrase TEXT NOT NULL, replacement TEXT, team_dictionary_id TEXT DEFAULT '00000000-0000-0000-0000-000000000000', last_used REAL, frequency_used INTEGER DEFAULT 0, manual_entry INTEGER DEFAULT 0, created_at REAL NOT NULL, modified_at REAL NOT NULL, is_deleted INTEGER DEFAULT 0, source TEXT, is_snippet INTEGER DEFAULT 0, and the constraint UNIQUE(phrase, team_dictionary_id).|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-013|`polish` is created with columns id TEXT PRIMARY KEY, initial_text TEXT, polished_text TEXT, initial_word_count INTEGER, polished_word_count INTEGER, app TEXT, processing_time REAL, status TEXT, polish_undone INTEGER DEFAULT 0, instruction TEXT, created_at REAL NOT NULL, updated_at REAL NOT NULL.|PolishStore.swift|wl-core::stores::polish|same|same|fixture|done|
|DB-014|`notes` is created with columns id TEXT PRIMARY KEY, title TEXT NOT NULL, content_preview TEXT NOT NULL, content TEXT NOT NULL, created_at REAL NOT NULL, modified_at REAL NOT NULL, is_deleted INTEGER DEFAULT 0, finalized INTEGER DEFAULT 0.|NotesStore.swift|wl-core::stores::notes|same|same|fixture|done|
|DB-015|Table creation order follows store construction order: HistoryStore, DictionaryStore, PolishStore, NotesStore.|AppDelegate.swift|wl-core::db|same|same|fixture|done|
|DB-016|Every `id` is an uppercase Foundation-format UUID string such as `E621E1F8-C36C-495A-93FC-0C247A3E6E5F`; `transcripts.id` comes from the transcription client and `polish.id` from the polish service.|DatabaseManager.swift|wl-core::db|same|same|fixture|done|
|DB-017|Every timestamp column is REAL holding Unix epoch SECONDS as a Double — not milliseconds and not the Apple reference date.|DatabaseManager.swift|wl-core::db|same|same|fixture|done|
|DB-018|Every boolean column is INTEGER 0 or 1.|DatabaseManager.swift|wl-core::db|same|same|fixture|done|
|DB-019|History insert is `INSERT OR REPLACE INTO transcripts (id, asr_text, formatted_text, timestamp, app_name, app_bundle_id, duration, num_words, language) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);` with `language` defaulting to `en`.|HistoryStore.swift|wl-core::stores::history|same|same|fixture|done|
|DB-020|History insert binds `app_name` to `appInfo["name"] ?? ""` and `app_bundle_id` to `appInfo["bundle_id"] ?? ""`, and `num_words` as a 32-bit integer.|HistoryStore.swift|wl-core::stores::history|same|same|fixture|done|
|DB-021|History `timestamp` is the wall clock at INSERT time, not the moment recording started.|HistoryStore.swift|wl-core::stores::history|same|same|unit|done|
|DB-022|`getEntries(limit:offset:)` runs `SELECT * FROM transcripts ORDER BY timestamp DESC LIMIT ? OFFSET ?;` with defaults limit 100, offset 0.|HistoryStore.swift|wl-core::stores::history|same|same|fixture|done|
|DB-023|History row decoding relies on declaration column order 0..8 in the Swift original; the port names the columns explicitly, which is behavior-identical and strictly safer.|HistoryStore.swift|wl-core::stores::history|DEVIATION: explicit column list replaces positional SELECT *|DEVIATION: explicit column list replaces positional SELECT *|fixture|done|
|DB-024|History search runs `SELECT * FROM transcripts WHERE formatted_text LIKE ? OR asr_text LIKE ? ORDER BY timestamp DESC LIMIT 100;` with the pattern `%<query>%` bound to both parameters.|HistoryStore.swift|wl-core::stores::history|same|same|fixture|done|
|DB-025|LIKE patterns are NOT escaped, so `%` and `_` typed by the user act as wildcards; case-insensitivity is SQLite's ASCII-only default.|HistoryStore.swift|wl-core::stores::history|same (preserved deliberately)|same (preserved deliberately)|unit|done|
|DB-026|`deleteEntry(id:)` is a HARD delete: `DELETE FROM transcripts WHERE id = ?;`|HistoryStore.swift|wl-core::stores::history|same|same|unit|done|
|DB-027|`clearAll()` runs `DELETE FROM transcripts;` with no WHERE clause and no soft-delete.|HistoryStore.swift|wl-core::stores::history|same|same|unit|done|
|DB-028|RETIRED as dead code: `SELECT COUNT(*), COALESCE(SUM(num_words), 0) FROM transcripts WHERE timestamp >= ?` bound to LOCAL-timezone midnight, returning (0, 0) on failure. Grep of the Swift tree finds exactly one occurrence — its own definition at HistoryStore.swift:99 — and zero call sites; no tray item, settings pane, history view or overlay state displays a daily counter. Not ported.|HistoryStore.swift:99|wl-core::stores::history|DEVIATION DV9|DEVIATION DV9|n/a|n/a|
|DB-029|History row mapping substitutes empty string for a missing id, appName or appBundleId and `en` for a missing language, while asrText and formattedText stay optional.|HistoryStore.swift|wl-core::stores::history|same|same|fixture|done|
|DB-030|`addNote` runs `INSERT INTO notes (id, title, content_preview, content, created_at, modified_at) VALUES (?, ?, ?, ?, ?, ?);` with defaults title `Untitled` and content empty, and returns the new UUID EVEN IF the insert fails.|NotesStore.swift|wl-core::stores::notes|same|same|unit|done|
|DB-031|`content_preview` is the first 200 extended grapheme clusters of the content — not 200 bytes and not 200 UTF-16 units — recomputed on every update.|NotesStore.swift|wl-core::stores::notes|same|same (unicode-segmentation graphemes, take 200)|unit|done|
|DB-032|New notes have `created_at == modified_at == now`.|NotesStore.swift|wl-core::stores::notes|same|same|unit|done|
|DB-033|`updateNote` runs `UPDATE notes SET title = ?, content_preview = ?, content = ?, modified_at = ? WHERE id = ?;`|NotesStore.swift|wl-core::stores::notes|same|same|fixture|done|
|DB-034|Note delete is soft: `UPDATE notes SET is_deleted = 1, modified_at = ? WHERE id = ?;`|NotesStore.swift|wl-core::stores::notes|same|same|fixture|done|
|DB-035|`getNotes(limit:)` runs `SELECT id, title, content_preview, content, created_at, modified_at FROM notes WHERE is_deleted = 0 ORDER BY modified_at DESC LIMIT ?;` with default limit 100.|NotesStore.swift|wl-core::stores::notes|same|same|fixture|done|
|DB-036|Notes search runs `SELECT id, title, content_preview, content, created_at, modified_at FROM notes WHERE is_deleted = 0 AND (title LIKE ? OR content LIKE ?) ORDER BY modified_at DESC LIMIT 100;` and matches the FULL content, not the preview.|NotesStore.swift|wl-core::stores::notes|same|same|fixture|done|
|DB-037|The polish store is write-only: there is not a single SELECT against the `polish` table anywhere in the app.|PolishStore.swift|wl-core::stores::polish|same|same|unit|done|
|DB-038|`saveResult` runs `INSERT OR REPLACE INTO polish (id, initial_text, polished_text, initial_word_count, polished_word_count, app, processing_time, status, instruction, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'completed', ?, ?, ?);` — `status` is a hard-coded SQL literal, so failures are never persisted.|PolishStore.swift|wl-core::stores::polish|same|same|fixture|done|
|DB-039|`polish.polish_undone` is never written and stays at its default 0.|PolishStore.swift|wl-core::stores::polish|same|same|fixture|done|
|DB-040|Dictionary insert runs `INSERT OR IGNORE INTO dictionary (id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?);` with defaults source `manual` and manual_entry true.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-041|Re-adding an existing phrase is a silent no-op: it does NOT update the replacement and does NOT bump `modified_at`, yet a fresh UUID is generated on every attempt and the cache is invalidated regardless.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DB-042|`updateEntry` runs `UPDATE dictionary SET phrase = ?, replacement = ?, modified_at = ? WHERE id = ?;` then invalidates the caches.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-043|Dictionary delete is soft: `UPDATE dictionary SET is_deleted = 1, modified_at = ? WHERE id = ?;` then invalidate; soft-deleted rows keep occupying the UNIQUE(phrase) slot so the same phrase can never be re-added through the UI.|DictionaryStore.swift|wl-core::stores::dictionary|same (latent bug preserved)|same (latent bug preserved)|unit|done|
|DB-044|`getVocabularyPhrases(limit:)` runs `SELECT phrase FROM dictionary WHERE is_snippet = 0 AND is_deleted = 0 ORDER BY frequency_used DESC LIMIT ?;` with default limit 50.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-045|`getReplacements()` runs `SELECT phrase, replacement FROM dictionary WHERE is_snippet = 0 AND replacement IS NOT NULL AND is_deleted = 0;` with NO limit.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-046|`getSnippets()` runs `SELECT phrase, replacement FROM dictionary WHERE is_snippet = 1 AND is_deleted = 0;` with no limit and no ORDER BY, skipping rows whose replacement is NULL.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-047|`fetchEntries(snippet:)` runs `SELECT id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at FROM dictionary WHERE is_snippet = ? AND is_deleted = 0 ORDER BY modified_at DESC;` with no limit.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-048|`searchEntries(query:snippet:)` runs the same projection with `AND phrase LIKE ?` and pattern `%<query>%`, searching `phrase` only and never `replacement`.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-049|`dictionary.last_used` is declared but never written or read.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-050|`dictionary.frequency_used` is only ever written as the literal 0 on insert and never incremented, so `ORDER BY frequency_used DESC` is effectively insertion-order today.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-051|`dictionary.team_dictionary_id` is never bound explicitly, so every row takes the default all-zero UUID and the UNIQUE constraint behaves as UNIQUE(phrase).|DictionaryStore.swift|wl-core::stores::dictionary|same|same|fixture|done|
|DB-052|`notes.finalized` is declared but never written or read.|NotesStore.swift|wl-core::stores::notes|same|same|fixture|done|
|DB-053|Indexes are created on `transcripts.timestamp`, `notes.modified_at` and `dictionary.deleted_at` so history paging is not a full table scan.|DatabaseManager.swift|wl-core::db|DEVIATION DV5 (Swift has zero indexes on all four tables)|DEVIATION DV5|unit|done|
|DB-054|Settings live in a single pretty-printed JSON file at `~/Library/Application Support/WisprLightning/settings.json`; UserDefaults is not used for settings at all.|Settings.swift|wl-core::settings|same|`%APPDATA%\WisprLightning\settings.json`|fixture|done|
|DB-055|A settings file that is missing, unreadable, or fails to decode falls back FIELD BY FIELD to defaults and the unparsable file is backed up, rather than silently resetting every setting.|Settings.swift|wl-core::settings|DEVIATION DV3 (Swift wipes all settings on any decode error)|DEVIATION DV3|unit|done|
|DB-056|JSON key names are the property names verbatim — no key-encoding strategy and no custom coding keys — and key order in the written file is unspecified because sorted keys are not requested.|Settings.swift|wl-core::settings|same|same|fixture|done|
|DB-057|`save()` encodes, re-parses and re-serializes pretty-printed (falling back to compact output on failure), writes non-atomically with all errors swallowed, and then unconditionally posts the settings-changed event.|Settings.swift|wl-core::settings|same|same|unit|done|
|DB-058|The four internal event-bus topics are named exactly `WisprLightningSettingsChanged`, `WisprSessionChanged`, `WisprPreviewSoundPack`, `WisprAudioDevicesChanged`.|Settings.swift|src-tauri::orchestrator|same|same|unit|done|
|DB-059|At stop the raw PCM buffer is written to disk before transcription so an unsent recording survives a crash.|AppDelegate.swift|wl-core::wav|same|same|e2e|done|
|DB-060|The WAV header builder is used ONLY for those on-disk debug/recovery dumps and never for the socket payload.|AppDelegate.swift|wl-core::wav|same|same|fixture|done|
|DB-061|Word counts everywhere use a literal-space split with empty subsequences omitted, which differs from general whitespace splitting on tabs and newlines.|TranscriptionClient.swift|wl-core::postproc|same|same|unit|done|
|DB-062|A Rust process opens a database written by the Swift reference implementation and reads every row of all four tables correctly.|n/a (new)|wl-core::db|same|same|fixture|done|

## 11. Dictionary & auto-learn

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|DIC-001|A dictionary row is a VOCABULARY phrase when `is_snippet = 0` and `replacement IS NULL`, and is sent to the server as an entry in `dictionary_context`.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-002|A row with `is_snippet = 0` and a non-NULL replacement is a REPLACEMENT pair, sent as `dictionary_replacements`.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-003|A row with `is_snippet = 1` and a non-NULL replacement is a SNIPPET, sent as `dictionary_snippets` with the expansion wrapped in a single-element array.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-004|A vocabulary row that HAS a replacement appears in BOTH `dictionary_context` (phrase only) and `dictionary_replacements` — the two queries overlap by design.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-005|Three independent optional caches back vocabulary, replacements and snippets; each is populated lazily on first call.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-006|`invalidateCache()` nils ALL THREE caches and is called only by add, update and soft-delete.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-007|The caches are read from the transcription auth path while the UI mutates them, so they are guarded by a read-write lock; the observable contract is only that caches invalidate on add, update and soft-delete.|DictionaryStore.swift|wl-core::stores::dictionary|DEVIATION DV6 (Swift reads the caches cross-thread with no lock)|DEVIATION DV6|unit|done|
|DIC-008|At startup the app seeds defaults and then primes all three caches so the first dictation does not pay the query cost.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|DIC-009|`seedDefaults(userName:)` adds the user's name as a vocabulary entry with `source = "default"` and `manual_entry = false` when the name is non-nil and non-empty.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-010|`seedDefaults` then unconditionally adds the phrase `Wispr Lightning` with `source = "default"` and `manual_entry = false`.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-011|The `source` column takes exactly four values: `manual` (default), `csv_import`, `user_edits` (auto-learned) and `default` (seeded).|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-012|CSV import that cannot read the file as UTF-8 returns `(0, ["Failed to read file"])`.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-013|CSV lines are split on the full newline character set, covering LF, CRLF, CR and U+2028/U+2029.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-014|Each CSV line is whitespace-trimmed and empty lines are skipped with no error and no counter increment.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-015|Only line index 0 can be a header, and only when its lowercased text contains `phrase` or `abbreviation`.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-016|A CSV line splits on `,`: field 0 trimmed of whitespace then of `"` is the phrase; the remaining fields re-joined with `,` and trimmed the same way are the replacement, which is nil when there is only one field — so replacements may legitimately contain commas.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-017|An empty phrase records the error `Line <index + 1>: empty phrase` and processing continues with the next line.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-018|`isSnippet` is set to `replacement != nil`, so ANY two-column CSV row becomes a snippet and never a replacement.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-019|Each imported row is added with `source = "csv_import"` and manual_entry true, and the imported counter increments even when the insert was dropped as a duplicate.|DictionaryStore.swift|wl-core::stores::dictionary|same|same|unit|done|
|DIC-020|The `Line <n>: invalid format` error branch is unreachable because the split always yields at least one element; it is not ported.|DictionaryStore.swift|wl-core::stores::dictionary|dead branch, not ported|dead branch, not ported|unit|done|
|DIC-021|`addAutoLearnedWord(phrase:)` adds a vocabulary entry with nil replacement, `source = "user_edits"` and `manual_entry = false`.|DictionaryStore.swift|wl-core::autolearn|same|same|unit|done|
|DIC-022|`addAutoLearnedWords(phrases:)` returns immediately for an empty list, otherwise wraps the whole loop in one BEGIN/COMMIT transaction with no rollback, invalidating the cache on each iteration.|DictionaryStore.swift|wl-core::autolearn|same|same|unit|done|
|DIC-023|Auto-learn runs only when `settings.autoLearnWords` is true AND both the ASR text and the formatted text are non-nil, i.e. AI formatting produced a different text.|AppDelegate.swift|wl-core::autolearn|same|same|unit|done|
|DIC-024|Auto-learn builds the candidate set as the lowercased literal-space split of the ASR text, then walks the literal-space split of the formatted text.|AppDelegate.swift|wl-core::autolearn|same|same|unit|done|
|DIC-025|A formatted word already present (case-insensitively) in the ASR words is skipped as not a correction.|AppDelegate.swift|wl-core::autolearn|same|same|unit|done|
|DIC-026|A candidate is trimmed of punctuation characters and kept only when the cleaned length is greater than 2 AND its first character is uppercase.|AppDelegate.swift|wl-core::autolearn|same|same|unit|done|
|DIC-027|A non-empty candidate list is written in one batch and logs `Auto-learned <n> words`; duplicates inside the batch and against existing rows are absorbed by INSERT OR IGNORE.|AppDelegate.swift|wl-core::autolearn|same|same|unit|done|
|DIC-028|`DictionaryEntry` equality and hashing are id-only, so two entries with identical text but different ids are distinct and an edited entry stays equal to its previous value.|DictionaryEntry.swift|wl-core::stores::dictionary|same|same|unit|done|

## 12. Tray / menu bar

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|TRY-001|With `showInDock` false (the default) the status-bar item is the app's only persistent UI surface and the app runs as an accessory.|AppDelegate.swift|src-tauri::tray|same|system tray icon; the app is hidden from the taskbar|e2e|done|
|TRY-002|`statusBar.setRecording(true)` is called during the press sequence, after the overlay is shown, so the menu-bar icon reflects an active recording.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-003|`statusBar.setRecording(false)` is called during the stop sequence, immediately after the stop sound.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-004|On system sleep while recording, the status-bar recording indicator is turned off as part of the abort sequence.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-005|When the Wispr Flow session watcher adopts a session, the tray is rebuilt — not by the watcher directly, but through `lib::watch_session`'s subscription, which is the SINGLE publisher of `session:changed`. One publisher means the tray cannot be refreshed by one path and missed by another.|AppDelegate.swift|src-tauri::tray|same|n/a — the watcher is macOS-only|e2e|done|
|TRY-006|The status-bar menu is refreshed on every audio-devices-changed event, including while recording.|AppDelegate.swift|src-tauri::tray|same|same|unit|done|
|TRY-007|The status-bar menu exposes a pause toggle that calls `setPaused(_:)`, so a user can suppress the hotkey without quitting.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-008|The menu-bar icon is `Resources/WisprFlowIcon.png` rendered at 18pt and is explicitly NOT a template image, so it keeps its own colors instead of tinting with the menu-bar appearance.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-009|Toggling `showInDock` changes the activation policy immediately, without a relaunch.|SettingsWindow.swift|src-tauri::tray|NSApp.setActivationPolicy regular or accessory|skipTaskbar toggled; taskbar semantics differ from the Dock|e2e|done|
|TRY-010|The tray menu items appear in exactly this order: last-transcription preview, separator, `Input Device` submenu, the pause/resume item, `Natural Mode`, `Settings`, separator, `Quit Wispr Lightning`.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-011|Menu item 1 previews the last transcription and copies that text to the clipboard when clicked.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-012|With no transcription yet, menu item 1 is the DISABLED item `No recent dictation` and clicking it does nothing.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-013|A separator follows the last-transcription preview item.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-014|The `Input Device` submenu lists `System Default`, a separator, then one CHECKABLE item per enumerated input device, with the check mark following the current `mic_device_id` and sitting on `System Default` when it is None; the check is never placed by matching `mic_device_name`.|AppDelegate.swift|src-tauri::tray|same|WASAPI endpoint ids enumerated the same way|e2e|done|
|TRY-015|Choosing a device from the submenu writes the pair (`mic_device_id`, `mic_device_name`) by calling INTO the settings store rather than writing settings itself, so the tray and the settings picker (SET-034) cannot diverge — the settings store is the single writer, `mic_device_id` is the sole resolution key and `mic_device_name` is a display label only.|AppDelegate.swift|wl-core::settings|same|same|unit|done|
|TRY-016|The pause item's title flips between `Pause hotkey` and `Resume hotkey`, is CHECKED while paused, and toggling it calls `setPaused(_:)` which persists `hotkeyPaused`.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-017|`Natural Mode` is a checkable menu item backed by the SAME `Settings::natural_mode_enabled` field as the settings toggle (SET-056), written through the settings store as single writer, so the two control surfaces cannot drift.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-018|`Settings` opens the settings window and carries the accelerator Cmd+comma.|AppDelegate.swift|src-tauri::tray|Cmd+,|Ctrl+,|e2e|done|
|TRY-019|A separator precedes the quit item.|AppDelegate.swift|src-tauri::tray|same|same|e2e|done|
|TRY-020|`Quit Wispr Lightning` terminates the app, running the full terminate sequence: close the history store, close the database handle, cancel the session-file watcher.|AppDelegate.swift|src-tauri::tray|same|same (no watcher to cancel)|e2e|done|
|TRY-021|A `settings.json` carried from macOS holds a `coreaudio:<uid>` in `mic_device_id` that matches no WASAPI endpoint; resolution must fall back to the system default input AND move the submenu check mark onto `System Default`, never leaving every item unchecked and never falling back to matching the stored `mic_device_name` against enumerated devices.|AppDelegate.swift|wl-platform::audio|n/a — a CoreAudio id always resolves on the machine that wrote it|fallback to System Default with the check mark moved there|probe|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|TRY-022|The tray's last-transcription LABEL collapses interior newlines to single spaces before eliding at 60 characters. Display only — clicking still copies the full untransformed text (TRY-011). A raw newline renders as a second line in an NSMenuItem and as a box glyph in a Win32 menu, so the verbatim string is unusable as a label.|n/a (new)|src-tauri::tray|DEVIATION (accepted): newlines collapsed for display|DEVIATION (accepted): newlines collapsed for display|unit|done|

## 13. Recording overlay

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|OVL-001|The overlay is a non-activating floating panel with an initial content rect of 120 x 36 and a full-size content view with no titlebar.|RecordingOverlay.swift|src-tauri::windows|tauri-nspanel 2.1.0 panel, focusable(false)|tauri window with focusable(false) giving WS_EX_NOACTIVATE|e2e|done|
|OVL-002|The overlay is shown with an order-front call and NEVER with a make-key call, and the app never activates itself for it, so it can never take keyboard focus away from the app being dictated into.|RecordingOverlay.swift|src-tauri::windows|DECISION D3: focusable(false) plus tauri-nspanel; window.show() would call makeKeyAndOrderFront and steal focus|DECISION D3: focusable(false) gives WS_EX_NOACTIVATE|e2e|done|
|OVL-003|The overlay sits above normal windows at a floating level.|RecordingOverlay.swift|src-tauri::windows|NSPanel gets NSStatusWindowLevel 25 via tauri-nspanel; tao always_on_top is only level 3|always-on-top plus WS_EX_TOOLWINDOW|e2e|done|
|OVL-004|The overlay window is fully transparent with a clear background, and the drop shadow is drawn by the FRONTEND in CSS with the window built shadow-disabled. AppKit's own shadow is drawn around an opaque frame, so on a transparent window it renders around the pill's bounding box rather than its rounded outline — the faithful-looking option is the wrong-looking one.|RecordingOverlay.swift|ui/overlay|DEVIATION (accepted): window shadow off, CSS shadow instead|DEVIATION (accepted): window shadow off, CSS shadow instead|e2e|done|
|OVL-005|The overlay is not draggable by its background.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-006|The overlay joins all Spaces and is stationary, so it is visible on every virtual desktop and ignores Expose.|RecordingOverlay.swift|src-tauri::windows|same|No analogue: the overlay appears only on the desktop where it was created unless re-created on desktop switch|manual: switch virtual desktops mid-recording on Windows and note whether the overlay follows|blocked — needs Windows hardware|
|OVL-007|Buttons inside the overlay remain clickable even though the window never activates.|RecordingOverlay.swift|src-tauri::windows|AppKit routes mouse events to non-activating panels|WS_EX_NOACTIVATE windows still receive clicks; must be verified for Retry/Save/dismiss|e2e|done|
|OVL-008|The overlay content is a frosted popover-material background with corner radius 18 and masked bounds, filling the panel.|RecordingOverlay.swift|ui/overlay|same (vibrancy)|CSS backdrop-filter blur(30px) saturate(180%) or Mica/Acrylic; solid fallback on Windows 10|e2e|done|
|OVL-009|Inside sits a horizontal stack pinned to all edges with spacing 8 and edge insets top 0, left 16, bottom 0, right 16.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-010|Stack child 1 is a 10 x 10 dot with corner radius 5 (a perfect circle) whose default fill is system red.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-011|Stack child 2 is a small indeterminate 16 x 16 spinner, initially hidden.|RecordingOverlay.swift|ui/overlay|NSProgressIndicator equivalent|CSS keyframe rotation on an SVG arc|e2e|done|
|OVL-012|Stack child 3 is the main label at 13pt body font in the primary label color, with initial text `Listening`.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-013|Stack child 4 is the elapsed-time label at body font in the secondary label color, initially hidden.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-014|Stack child 5 is a small rounded `Retry` button, initially hidden.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-015|Stack child 6 is a small rounded `Save` button, initially hidden.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-016|Stack child 7 is a borderless inline dismiss button titled `✕` (U+2715), initially hidden.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-017|The overlay is always positioned bottom-center of the main screen's visible frame: `x = visibleFrame.midX - width/2`, `y = visibleFrame.minY + 50`, and its height stays 36 in every state. Measured widths: Recording 120, Locked 120, Processing 145, Retrying 175, Error 180, Recoverable 300, elapsed-visible 200. Positioning is owned by `src-tauri::windows`, not the webview.|RecordingOverlay.swift|src-tauri::windows|same|monitor work area from SPI_GETWORKAREA; y measured from the work-area bottom|e2e|done|
|OVL-018|Resizing is a no-op when the requested width already matches the current width; `show()` zeroes the tracked width first to force a reposition, which is how the panel re-centers after a wide error state.|RecordingOverlay.swift|src-tauri::windows|same|same|unit|done|
|OVL-019|`hide()` orders the panel out, resets the dot to red and visible, stops and hides the spinner.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-020|`show()` renders width 120, a pulsing red dot, hidden spinner, label `Listening`, no background tint, and never auto-dismisses.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-021|`showLocked()` renders width 120, a still-pulsing SYSTEM GREEN dot, label `Recording`, no tint, no auto-dismiss.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-022|`showProcessing()` renders width 145, dot hidden with the pulse stopped, spinner visible and animating, label `Processing`, no tint.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-023|`showRetrying(attempt:maxAttempts:)` renders width 175, no dot, spinner animating, label `Retrying… (N/M)` with a U+2026 ellipsis, and a system-yellow background tint at 20% opacity.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-024|`showError(message:)` renders width 180, no dot, no spinner, the message as the label, a system-red tint at 30% opacity, and auto-hides after exactly 3000 ms. The webview owns the visual state; the generation-guarded 3000 ms auto-hide and the window ordering-out live in `src-tauri::windows`, because a webview cannot orderOut its own window.|RecordingOverlay.swift|ui/overlay + src-tauri::windows|same|same|e2e|done|
|OVL-025|RETIRED as dead code: the `showRetryableError` 260 px no-Save variant at RecordingOverlay.swift:224, reached only via the defaulted `onSave: (() -> Void)? = nil`. All four call sites (AppDelegate.swift:588, 601, 635, 782) pass `onSave`, so the branch is UNREACHABLE rather than unimplemented — the distinction matters, because unimplemented invites someone to finish it. Independently, audio is spooled to disk on every path before transcription, so Save is always offerable and a Recoverable without it would offer strictly less than the app can do. `OverlayState::Recoverable` therefore carries no has-save-handler flag. Not ported.|RecordingOverlay.swift:224|ui/overlay|DEVIATION DV12|DEVIATION DV12|n/a|n/a|
|OVL-026|`showRetryableError` renders at width 300 with a red 30% tint and never auto-dismisses. 300 is the ONLY Recoverable width because Save is ALWAYS offered: `OverlayState::Recoverable { message }` deliberately carries no save flag, `overlay_action` always accepts `save`, and adding a `can_save` field to the enum is explicitly ruled out.|RecordingOverlay.swift:224|ui/overlay|same|same|e2e|done|
|OVL-027|`showWarning()` leaves width, dot, spinner and label unchanged and only applies a system-yellow 30% background tint.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-028|`showFinalWarning()` leaves width, dot, spinner and label unchanged and only applies a system-red 30% background tint.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-029|There is no success or Done state — a completed dictation simply hides the overlay.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-030|The only motion during recording is one opacity pulse on the 10pt dot: 1.0 to 0.3, duration 0.6 s, auto-reversing (full cycle 1.2 s), repeating forever, ease-in-ease-out; stopping it removes the animation and forces opacity back to 1.0.|RecordingOverlay.swift|ui/overlay|same|CSS `animation: pulse 1.2s ease-in-out infinite`|e2e|done|
|OVL-031|There is no waveform, no level meter and no bars anywhere in the overlay; adding one would be a new feature, not parity.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-032|`updateElapsed(seconds:)` returns immediately for values below 30, so the timer is invisible for the first 30 seconds and the panel then jumps from 120 to 200 px wide and re-centers.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-033|Elapsed time renders as `%d:%02d` (for example `0:30`, `9:05`), with ` ⚠️` (space + U+26A0 U+FE0F) appended whenever the warning state is above 0.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-034|The warning state is monotonic 0 to 1 to 2: `showWarning()` no-ops at 1 or above, `showFinalWarning()` no-ops at 2, and `show()`, `showLocked()` and `showProcessing()` each reset it to 0.|RecordingOverlay.swift|ui/overlay|same|same|unit|done|
|OVL-035|The Retry button invokes the retry callback.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-036|The Save button invokes the save callback, then changes its own title to `Saved` and disables itself until the next `show()`.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-037|The dismiss button invokes the dismiss callback.|RecordingOverlay.swift|ui/overlay|same|same|e2e|done|
|OVL-038|`show()` and `hide()` both clear all three callbacks and reset the Save button to title `Save` and enabled.|RecordingOverlay.swift|ui/overlay|same|same|unit|done|
|OVL-039|Overlay messages authored by the orchestrator are exactly `Mic unavailable`, `Mic not responding`, `Select text to polish`, `Timed out` and `Recovered unsent recording`.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|OVL-040|Retryable errors — connectionFailed, timeout and serverError — get the Retry/Save/dismiss treatment; non-retryable errors — authFailed and emptyResult — get the transient 3 s toast-style presentation.|AppDelegate.swift|wl-core::fsm|same|same|unit|done|
|OVL-041|The overlay panel is constructed at app launch without being shown, so the first hotkey press pays no construction latency.|RecordingOverlay.swift|src-tauri::windows|same|same|e2e|done|
|OVL-042|The toast notification surface is not ported: it is constructed in the Swift app but `show(wordCount:)` is never invoked anywhere.|ToastNotification.swift:105|ui/overlay|DEVIATION DV7 (dead code)|DEVIATION DV7 (dead code)|unit|done|
|OVL-043|Showing the overlay must not change which window has keyboard focus; this invariant is asserted directly rather than inferred from a successful injection.|RecordingOverlay.swift|src-tauri::windows|same|same|e2e|done|

## 14. Settings UI (one row per control)

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|SET-001|The settings window opens at a content rect of 860 x 580 points.|SettingsWindow.swift|src-tauri::windows|same|same at 100% DPI; fractional 125%/150% scaling requires rem-based sizing|e2e|done|
|SET-002|The settings window has a minimum size of 680 x 460.|SettingsWindow.swift|src-tauri::windows|same|same|e2e|done|
|SET-003|The window is titled, closable, miniaturizable, resizable, with a full-size content view.|SettingsWindow.swift|src-tauri::windows|same|same minus miniaturizable semantics|e2e|done|
|SET-004|The window title is exactly `Wispr Lightning Settings`.|SettingsWindow.swift|src-tauri::windows|same|same|e2e|done|
|SET-005|The titlebar is opaque with a visible title and a unified toolbar style.|SettingsWindow.swift|src-tauri::windows|same|hand-rolled header bar; native unified toolbar has no Windows twin|e2e|done|
|SET-006|The window centers itself on first show only.|SettingsWindow.swift|src-tauri::windows|same|same|e2e|done|
|SET-007|Window position and size are persisted under the frame autosave name `SettingsWindow`.|SettingsWindow.swift|src-tauri::windows|same|persisted via the window-state plugin|e2e|done|
|SET-008|The window instance is reused rather than released on close.|SettingsWindow.swift|src-tauri::windows|same|same|e2e|done|
|SET-009|Re-showing the settings window makes it key and activates the app but does NOT rebuild the view model in the Swift original, so settings edited on disk elsewhere are not re-read until relaunch; the reactive store in the port does re-read. NOTE the activation half is NOT yet proven: it is implemented on the main thread after the window is shown, but has no effect for an UNBUNDLED binary, because such a process has no bundle identifier and macOS refuses to activate it — System Events' own `set frontmost to true` fails on it too. The signed bundle (LIF-022) now exists, so the prerequisite is satisfied and only the re-check remains.|SettingsWindow.swift|ui/settings|DEVIATION: the port picks up external settings.json changes live; activation pending bundled re-check|DEVIATION: the port picks up external settings.json changes live|e2e|blocked — signed bundle now exists (LIF-022); awaiting the activation re-check from inside it|
|SET-010|The sidebar is a fixed 220-point column using the native sidebar list style, with the platform sidebar-toggle button removed.|SettingsWindow.swift|ui/settings|same|hand-rolled 220 px flex column|e2e|done|
|SET-011|The sidebar top inset shows the bundled `WisprFlowIcon.png` at 64 x 64 clipped to a rounded rect of radius 14, horizontally centered, with 16 top padding and 8 bottom padding on a clear background.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-012|The sidebar has three unlabeled groups rendered as separator gaps with no headers. Group 1 has FOUR rows: General, Dictation, Transcription, Polish — `Transcription` is the new provider pane, has no Swift original, and sits in group 1 because that is where the other what-happens-to-your-voice controls live. Group 2 is History, Dictionary, Notes and group 3 is Privacy, System, both unchanged.|SettingsWindow.swift|ui/settings|DEVIATION (accepted): Swift group 1 has three rows|DEVIATION (accepted): Swift group 1 has three rows|e2e|done|
|SET-013|Each sidebar row pairs a 28 x 28 gradient icon tile of corner radius 7 with the section title and 1 point of vertical padding; the eight gradients are Gray #A3A3B3->#7A7A8C (General, System), Blue #4D91FF->#2461F5 (Dictation, Privacy), Purple #B861FF->#8C38F0 (Polish), Orange #FFAD38->#FA8005 (History), Green #57D170->#33B34D (Dictionary), Yellow #FFD62E->#FAB30A (Notes).|SettingsWindow.swift|ui/settings|same, SF Symbols glyphs|SF Symbols are not licensable, so a substituted icon set is used and row heights shift slightly|e2e|done|
|SET-014|History, Dictionary and Notes fill the detail pane edge to edge with no scroll wrapper and no 28-point padding; every other section is a scroll view containing a leading-aligned vertical stack with spacing 16 and padding 28.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-015|The default selected section is General.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-016|The account block refreshes on appear and on every session-changed event.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-017|Reusable toggle rows render a switch-style toggle at small control size whose label is a leading vertical stack with spacing 2: title in body font, optional description in subheadline regular weight and secondary color, with the switch flush right.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-018|General > Account signed-in: the avatar is a 32 x 32 circle-clipped scaled-to-fill remote image; with no URL or while loading it falls back to the person-circle glyph at title2 size in secondary color.|SettingsWindow.swift|ui/settings|same|substituted glyph|e2e|done|
|SET-019|General > Account signed-in: the display name is `[firstName, lastName]` filtered non-empty and joined with a space, falling back to the email, and is rendered in medium-weight body font ONLY when it is non-empty AND different from the email.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-020|General > Account signed-in: the email renders below the name in caption font, secondary color.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-021|General > Account signed-in: a small `Sign Out` button clears the session and posts session-changed.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-022|General > Account signed-out: a person-with-question-mark glyph at title2 secondary plus the text `Not signed in`.|SettingsWindow.swift|ui/settings|same|substituted glyph|e2e|done|
|SET-023|General > Account signed-out: a small `Sign In with Google` button opens the authorize URL in the system browser.|SettingsWindow.swift|ui/settings|same|same; if the `wispr-flow` scheme is owned by another app, the user completes sign-in through the `Paste sign-in link` fallback|e2e|done|
|SET-024|General > Dictation Hotkeys: the group opens with the secondary-colored line `Any of these keys will start dictation:`.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-025|General > Dictation Hotkeys: each entry of `hotkeyLabels` (default `["Left Control"]`) renders a keycap — monospaced medium body text, min width 40, horizontal padding 12, vertical padding 6, control-background fill, corner radius 6, 1pt separator stroke.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-026|General > Dictation Hotkeys: a red borderless minus-circle button with tooltip `Remove this hotkey` appears next to each keycap ONLY when more than one hotkey is configured.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-027|General > Dictation Hotkeys: removing a hotkey also rewrites the legacy `hotkeyKeyCode` and `hotkeyLabel` from index 0 of the remaining list.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-028|General > Dictation Hotkeys: a small button titled `Add Hotkey`, changing to `Press a key…` while capturing, calls `hotkey_capture_begin` on press and `hotkey_capture_end` on the next key event; capture is performed by the platform `HotkeyBackend`, NOT by webview `keydown`/`keyup`, because on macOS the Fn key never reaches a webview at all and Fn (keycode 63) is a bindable hotkey. On flags-changed only a PRESS counts.|SettingsWindow.swift|wl-platform::hotkey|webview capture cannot see Fn, so capture lives in the backend|same backend path; no Fn virtual key exists to capture|e2e|done|
|SET-029|General > Dictation Hotkeys: the captured label is `keycodeLabels[keycode]`, else the uppercased characters-ignoring-modifiers, else `?`; the label is derived from the `Hotkey` returned by `end_capture`, never from a webview key event.|SettingsWindow.swift|ui/settings|same|portable Chord display name|unit|done|
|SET-030|General > Dictation Hotkeys: capturing a keycode that is already bound silently cancels the capture without adding a duplicate.|SettingsWindow.swift|ui/settings|same|same|unit|done|
|SET-031|General > Dictation Hotkeys: the group footer reads `Modifier keys work as hold-to-talk. Regular keys use press-to-toggle.` in subheadline tertiary.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-032|General > Input Device: a label-hidden pop-up picker bound to `micDeviceId` whose first option is `System Default` with tag nil (the default).|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-033|General > Input Device: one picker row per enumerated device, labeled with the device name and tagged with its cpal `DeviceId` string (`coreaudio:<uid>` or `wasapi:<endpoint>`); the tag is what selection is matched on, the label is never matched.|SettingsWindow.swift|ui/settings|same|WASAPI endpoint ids; a `coreaudio:` id carried over from macOS matches nothing and resets to System Default|e2e|done|
|SET-034|General > Input Device: changing the picker writes the pair (`mic_device_id`, `mic_device_name`) through the settings store, which is the SINGLE WRITER of both. `mic_device_id` is the sole resolution key; `mic_device_name` is a display label used only to title the current selection before enumeration completes and to name the device in the unavailable-mic log line. Nothing ever matches on it, which is why two identical USB mics sharing a name is harmless.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-035|General > Input Device: a small `Refresh` button with a clockwise-arrow glyph re-enumerates devices.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-036|General > Input Device: toggle `Keep microphone active` with description `Eliminates startup delay — recommended when using iPhone as microphone`, key `keepMicrophoneActive`, default false, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-037|General > Dictation Languages: toggle `Auto-detect` with description `Automatically detect the spoken language`, bound to whether the selected set contains `auto`, default OFF because `languages` defaults to `["en"]`.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-038|General > Dictation Languages: when Auto-detect is ON the only content shown is the subheadline secondary text `All supported languages will be recognized automatically. Specifying languages manually can improve accuracy.` and everything below is hidden.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-039|General > Dictation Languages: when Auto-detect is OFF and at least one language is selected, chips render in a flow layout with spacing 6, each chip being `<flag> <name>` plus an xmark-circle remove button, padding 8 horizontal and 4 vertical, accent color at 12% background, corner radius 12.|SettingsWindow.swift|ui/settings|same|accent color extracted from the OS; shade differs so contrast is re-checked|e2e|done|
|SET-040|General > Dictation Languages: a rounded-border text field placeholder `Search languages...` filters case-insensitively on the language NAME only, never the code.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-041|General > Dictation Languages: the language list is a fixed-height 220 scroll region with a text-background fill, corner radius 6, 1pt separator stroke and visible scroll indicators; each row is a small switch labeled `<flag> <name>` with 8 horizontal and 5 vertical padding followed by a divider inset 8 from the leading edge.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-042|General > Dictation Languages: a non-interactive bottom fade overlay of height 28 blends clear to the text background at 85% opacity with corner radius 6.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-043|General > Dictation Languages: turning `auto` ON sets the selection to exactly `["auto"]`, wiping all other selections.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-044|General > Dictation Languages: turning `auto` OFF sets the selection to `["en"]`.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-045|General > Dictation Languages: toggling any specific code first removes `auto`, and toggling off the last remaining code resets to `["en"]`, so the selection is never empty.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-046|General > Dictation Languages: the language table has 104 entries in a fixed display order starting `en` English, `engb` English — British, `zh` Chinese — Traditional, `zhcn` Chinese — Simplified, `de` German, and ending `ba` Bashkir, `jv` Javanese, `su` Sundanese, `yue` Cantonese; chips render in this master-table order and not in selection order. The ui-spec prose says 101, but its own enumeration lists 104 unique codes with no duplicates — independently re-counted; the four codes a naive 2-3 character parse misses are `engb`, `zhcn`, `dech`, `hien`. The enumeration transcribes the Swift array; the prose count is arithmetic.|SettingsWindow.swift|ui/settings|DEVIATION (accepted): spec prose count corrected 101 -> 104|DEVIATION (accepted): spec prose count corrected 101 -> 104|unit|done|
|SET-047|Dictation > toggle `AI Formatting`, description `Apply AI formatting to clean up transcriptions`, key `aiFormatting`, default true, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-048|Dictation > segmented control with no visible label, options None -> `none`, Light -> `light`, Heavy -> `heavy`, key `autoCleanupLevel`, default `light`, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-049|Dictation > caption `How aggressively to clean up filler words` in subheadline secondary, directly under the cleanup segmented control.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-050|Dictation > toggle `Voice Commands`, description `Interpret phrases like "new line" as commands`, key `commandModeEnabled`, default true, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-051|Dictation > toggle `Auto-detect hyperlinks`, description `Convert spoken URLs to clickable hyperlinks`, key `hyperlinkOn`, default false, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-052|Dictation > toggle `Auto-learn words`, description `Automatically learn new vocabulary from dictations`, key `autoLearnWords`, default true, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-053|Dictation > toggle `Email signature`, description `Append a signature when dictating in email apps`, key `emailAutoSignature`, default false, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-054|Dictation > menu dropdown labeled `Signature`, options `Written with Wispr Lightning` -> `written_with_lightning` and `Spoken with Wispr Lightning` -> `spoken_with_lightning`, key `emailSignatureOption`, default `written_with_lightning`, ENTIRELY HIDDEN (not greyed) unless Email signature is on.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-055|Dictation > toggle `Creator mode`, description `Extended recording for long-form content (up to 10 min)`, key `creatorMode`, default false, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-056|Dictation > toggle `Natural Mode`, description `Type text character-by-character instead of pasting (slower but feels human)`, key `naturalModeEnabled` (`Settings::natural_mode_enabled`), default false, no dependency; the tray item TRY-017 is a second surface onto this same field, written through the settings store as single writer.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-057|Dictation > segmented control labeled `Typing speed`, options Slow -> `slow`, Normal -> `normal`, Expert -> `expert`, key `naturalModeSpeed`, default `normal`, hidden unless Natural Mode is on.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-058|Dictation > caption `Slow ≈ 30 WPM, Normal ≈ 50 WPM, Expert ≈ 80 WPM`, hidden unless Natural Mode is on.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-059|Dictation > Personalization toggle `Style detection`, description `Automatically adjust tone based on context`, key `styleDetectionEnabled`, default true, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-060|Dictation > Personalization menu dropdown labeled `Work`, key `personalizationStyles["work"]`, default `default`, hidden unless Style detection is on.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-061|Dictation > Personalization menu dropdown labeled `Email`, key `personalizationStyles["email"]`, default `default`, hidden unless Style detection is on.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-062|Dictation > Personalization menu dropdown labeled `Personal`, key `personalizationStyles["personal"]`, default `default`, hidden unless Style detection is on.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-063|Dictation > Personalization menu dropdown labeled `Other`, key `personalizationStyles["other"]`, default `default`, hidden unless Style detection is on.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-064|All four personalization dropdowns offer the same five raw values rendered capitalized: `default` Default, `formal` Formal, `casual` Casual, `friendly` Friendly, `professional` Professional.|SettingsWindow.swift|ui/settings|same|same|unit|done|
|SET-065|Polish > toggle `Enable Polish`, description `Refine selected text with AI`, key `polishEnabled`, default false, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-066|Polish > every control below Enable Polish is HIDDEN (not disabled) while Enable Polish is off.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-067|Polish > subheadline secondary label `Polish hotkey:` followed by one keycap per entry of `polishHotkeyLabels` (default `["Right Control"]`, keycode 62), each with a red borderless minus-circle button tooltipped `Remove this polish hotkey` shown only when more than one is configured.|SettingsWindow.swift|ui/settings|same|Right Control maps to VK_RCONTROL 0xA3|e2e|done|
|SET-068|Polish > small button titled `Add Polish Hotkey`, changing to `Press a key…` while capturing, driving the same `begin_capture`/`end_capture` backend path as the dictation hotkey and writing `polishHotkeyKeyCodes` and `polishHotkeyLabels`. This matters most here: the shipped default polish binding is the bare modifier Right Control, so a capture UI that cannot express bare modifiers cannot even re-bind the default.|SettingsWindow.swift|wl-platform::hotkey|same|same|e2e|done|
|SET-069|Polish > subheadline secondary label `Polish instructions:` above the instruction toggles.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-070|Polish > one small switch per instruction key, rendered in alphabetical key order with the key string as the label, defaults: `Add structure for readability` true, `Clarify main point` false, `Maintain your tone` true, `Make more concise` true, `Refine phrasing for impact` false, `Reorder for readability` true, `Reword for clarity` true.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-071|Polish > toggle `Auto-polish after dictation`, description `Automatically polish text after each dictation`, key `autoPolish`, default false, hidden unless Enable Polish is on.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-072|Privacy > toggle `Screen context (OCR)`, description `Capture screen text for context-aware formatting`, key `useScreenContext`, default false, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-073|Privacy > toggle `Accessibility context`, description `Use accessibility APIs for better transcription context`, key `useAccessibilityContext`, default true, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-074|Privacy > toggle `Share anonymous usage data`, description `Help improve Wispr by sharing anonymous statistics`, key `shareUsageData`, default false, no dependency.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-075|System > toggle `Launch at login` with no description, key `launchAtLogin`, default false; the side effect writes or removes `~/Library/LaunchAgents/com.wisprlightning.app.plist` with Label com.wisprlightning.app, ProgramArguments the executable path, RunAtLoad true, KeepAlive false, falling back to `/Applications/Wispr Lightning.app/Contents/MacOS/WisprLightning`.|SettingsWindow.swift|src-tauri::commands|tauri-plugin-autostart with MacosLauncher::LaunchAgent to avoid the TCC automation prompt|tauri-plugin-autostart; failures must surface in the UI because Run-key writes can be blocked by policy|probe|done|
|SET-076|System > toggle `Show in Dock`, key `showInDock`, default false; the side effect changes the activation policy immediately.|SettingsWindow.swift|src-tauri::tray|NSApp.setActivationPolicy regular or accessory|relabelled `Show in taskbar`; skipTaskbar toggled, semantics differ from the Dock|e2e|done|
|SET-077|System > toggle `Sound effects`, key `enableSounds`, default true, no side effect beyond gating playback.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-078|System > toggle `Mute music while dictating`, key `muteMusic`, default false.|SettingsWindow.swift|wl-core::settings|same|same|unit|done|
|SET-079|System > toggle `Verbose logging`, description `Log full server requests and responses to ~/Library/Logs/WisprLightning.log`, key `verboseLogging`, default false.|SettingsWindow.swift|wl-core::settings|same|description path becomes the Windows log location under %LOCALAPPDATA%|unit|done|
|SET-080|System > menu dropdown labeled `Sound pack`, key `selectedSoundPack`, default nil; the first option is `Default` with tag nil, followed by one capitalized entry per available pack EXCLUDING the literal `default`, so a missing Sounds folder leaves only `Default`.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-081|System > the small `Preview` button next to the sound-pack dropdown awaits `settings_save` and then calls `sound_preview`, so the reloaded pack is always the one previewed. This replaces the Swift sequence of posting settings-changed and firing the preview 200 ms later: same ordering guarantee, no timing guess. Observed invoke sequence is `settings_save` then `sound_preview`.|SettingsWindow.swift|ui/settings|DEVIATION (accepted): awaited ordering replaces the 200 ms timer|DEVIATION (accepted): awaited ordering replaces the 200 ms timer|e2e|done|
|SET-082|System > below the group, a divider then the tertiary subheadline text `Wispr Lightning v1.0.0`, which deliberately does not match `clientVersion` `1.4.549`.|SettingsWindow.swift|ui/settings|same|same|e2e|done|
|SET-083|Theme colors are semantic and flip with the system appearance: window background, secondary label, control accent, system red, tertiary label; spacing tokens are small 4, medium 8, large 16, xlarge 24; fonts are title3 15pt regular, headline 13pt semibold, body 13pt regular, subheadline 11pt regular.|Theme.swift|ui/settings|same|CSS custom properties plus prefers-color-scheme; accent from the OS colorization/UISettings accent|e2e|done|
|SET-084|Corner radii come from the app.css design tokens rather than literal call-site values, because hard-coded radii are forbidden and app.css defines no 6px or 12px token. Mapping: spec 6 (keycap, language list box, search fields) renders as `--radius-sm`, 5px on macOS and 4px on Windows; spec 12 (language chips) renders as `--radius-lg`, 10px and 8px; spec 14 (sidebar app icon) is exact via `--radius-icon`; spec 7 (section icon tile) is exact because it is an SVG geometry attribute, not CSS. Overlay 18, toast 12 and the vocabulary source badge 4 are unaffected.|Theme.swift|ui/settings|DEVIATION (accepted): --radius-sm 5px, --radius-lg 10px|DEVIATION (accepted): --radius-sm 4px, --radius-lg 8px|e2e|done|
|SET-085|Privacy > a System Permissions block lists each permission with its status from `Permissions::status()`. Known keys render as `Microphone`, `Accessibility`, `Input Monitoring`, `Screen Recording`, `Automation`; an unrecognised key is title-cased and shown rather than dropped. States render as `Granted`, `Denied`, `Not requested`, `Not required on this system`. Block caption: `Dictation needs these to hear you, to see the key you press, and to type into the app in front of you. A denial here is silent otherwise — the app simply never triggers.` This is the diagnostic whose absence HTK-032, LOG-012 and LIF-016 all describe.|n/a (new)|ui/settings + wl-platform::permissions|same UI; the backend reports the four TCC grants plus Automation|same UI; the backend reports Microphone and never reports not_determined|probe|done|
|SET-086|Privacy > System Permissions: the affordance follows `Permissions::status()` with NO platform branch in the UI — `Request Access` renders only for `not_determined` and invokes `Permissions::request(p)`; `granted` and `not_applicable` get no action at all.|n/a (new)|ui/settings + wl-platform::permissions|Accessibility and Microphone report not_determined, so the control appears|the backend never reports not_determined, so the control never appears — a consequence of the status, not a UI check|probe|done|
|SET-087|Privacy > System Permissions: `Open Settings` renders only for `denied` and deep-links to the OS privacy pane, so a denied permission is never shown as a status with no available action. On Windows the activation MUST run on a bounded STA worker: `ms-settings:` activation goes through COM, the shell publishes no marshalling info, and the tokio workers are already in the MTA (`ensure_mta` puts them there on every dictation) — so a shell call from an MTA fails for exactly the verbs involving a COM object. Left unfixed this silently fails to open the microphone settings page, removing the ONLY remediation a user has after being told microphone access is Denied.|n/a (new)|ui/settings + wl-platform::permissions|opens System Settings > Privacy & Security at the relevant pane|`ms-settings:privacy-microphone` activated from a bounded STA worker; MTA activation fails intermittently and silently|probe|done|
|SET-088|Transcription pane chrome: sidebar title `Transcription`, group header `Transcription Provider` for the picker and credential controls, group header `What this provider does` for the capability panel.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-089|Transcription > control `Provider`: segmented control with role radiogroup and a VISIBLE label, options `Wispr Flow` -> `wispr` and `Deepgram` -> `deepgram`, bound to `Settings::provider` (JSON key `provider`), default `wispr`, no dependency.|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-090|Transcription > the General > Account controls are mounted a SECOND time here, bound to `auth_status` / `auth_sign_in` / `auth_sign_out`, HIDDEN unless `provider == wispr`. It is the same component as General, so its literals are already pinned by SET-018 through SET-023 and cannot drift between the two panes.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-091|Transcription > control `API key`: password text field, placeholder `Paste your Deepgram API key`. It is NOT a settings field — it is write-only, submitted through `deepgram_key_save`, never returned by the backend and never displayed. Default empty and never populated from the backend. HIDDEN unless `provider == deepgram`; DISABLED while a save or clear is in flight.|Transcription.svelte|ui/settings|key lands in the macOS Keychain per DECISION D8|key lands in Windows Credential Manager per DECISION D8|probe|done|
|SET-092|Transcription > static text beside the `API key` label reads `Configured` or `Not configured`, driven by the `deepgram.configured` entry of `provider_list()`, defaulting to `Not configured`. Caption below reads `The key is stored in the system keychain and never shown again. Paste a new one to replace it.` HIDDEN unless `provider == deepgram`.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-093|Transcription > button `Save Key` (accent): calls `deepgram_key_save(key)` and on success clears the field and re-reads `provider_list`. DISABLED when the trimmed field is empty or a request is in flight.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-094|Transcription > button `Clear`: calls `deepgram_key_clear()`. DISABLED when no key is configured or a request is in flight.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-095|Transcription > control `Model`: pop-up menu with a visible label, options `Nova 3` -> `nova-3` and `Nova 2` -> `nova-2`, bound to `deepgramModel`, default `nova-3`. HIDDEN unless `provider == deepgram`.|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-096|Transcription > switch `Send vocabulary as recognition hints`, bound to `deepgramKeytermBoost`, default true, HIDDEN unless `provider == deepgram`. When `deepgramModel` STARTS WITH `nova-3` the control is ENABLED and its description reads `Biases recognition toward your dictionary phrases. Only the Nova 3 family supports this.` The family prefix is deliberate, not an exact id: `deepgramModel` is a free string and a settings file can legitimately carry `nova-3-medical`, which is Nova 3 and does support keyterm. Verified: `nova-3` and `nova-3-medical` enabled with this text.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-111|Transcription > switch `Send vocabulary as recognition hints` when `deepgramModel` does NOT start with `nova-3`: the control is DISABLED and STILL CHECKED whenever the stored value is true, and its description changes to name the responsible model — `Requires a Nova 3 model. <deepgramModel> does not support keyterm boosting, so your dictionary is not sent as recognition hints.` The stored value is deliberately NOT cleared when the model leaves the Nova 3 family, because that would destroy a preference during an unrelated edit; this second description is what stops on-but-grey reading as "this is doing something" and makes it read as a parked preference instead. Same data, no new control. Verified on `nova-2` with the stored value both true and false.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-097|Transcription > switch `Apply replacements and snippets locally`, description `Deepgram has no server-side formatter, so your dictionary's replacements and snippets are applied here after transcription. This is separate from AI Formatting, which only affects punctuation and tidy-up — turning that off does not stop your dictionary working.`, bound to `localPostProcessing`, default true. HIDDEN unless `provider == deepgram`.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-098|Transcription > a static language-mapping notice with role note renders exactly one of four mutually exclusive variants derived from `Settings::languages`, mirroring the mapping in `wl_providers::deepgram`. HIDDEN unless `provider == deepgram`.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-099|Transcription > language notice variant A, no languages selected: `No languages are selected, so Deepgram is asked for English.`|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-100|Transcription > language notice variant B, exactly one language selected: `Deepgram accepts one language per request, so your single selection (<name>) is the one it uses.` The phrasing deliberately avoids `sent as-is`, which would describe a bug: the provider TRANSLATES at the boundary (PRV-027), so with `zh` selected the sentence renders `...your single selection (Chinese — Traditional (繁體中文)) is the one it uses.` and is true, whereas `sent as-is` would promise the pass-through that PRV-031 shows returns a clean transcript in the wrong script. A code comment above the string records why, so it is not simplified back.|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-101|Transcription > language notice variant C, two or more selected: `Deepgram accepts one language per request, and you have several selected (<names>). They are sent as “multi”, Deepgram's code-switching mode, which spans English, Spanish, French, German, Hindi, Russian, Portuguese, Japanese, Italian and Dutch — a selected language outside that set will not be recognised.`|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-102|Transcription > language notice variant D, `languages == ["auto"]` and NOT (keyterm boost on AND `deepgramModel` starts with `nova-3`): the base string alone — `Auto-detect is on, so Deepgram detects the spoken language itself — across 35 languages, not every language offered above. Detection is weakest on short utterances, which is most of push-to-talk dictation, and if the detected language is not available on the model selected here Deepgram quietly falls back to an older one. Selecting the language you speak is still the more accurate choice.` The copy deliberately says `not every language offered above` rather than naming a count, so it cannot rot against SET-046 when either the 104-entry picker or Deepgram's 35 detected languages moves. This row must move with PRV-026 and PRV-028.|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-110|Transcription > language notice variant D-extended, `languages == ["auto"]` AND `deepgramKeytermBoost` AND `deepgramModel` starts with `nova-3`: the SET-102 base string plus, appended after a single space, `Your recognition hints are still sent, but that fallback may leave them with no effect: hints are a Nova 3 feature, and nothing in the response says which model actually ran.` The wording is deliberate — keyterms ARE still sent under auto-detect, so any phrasing implying they are disabled or skipped would be false; and the trailing clause is the sentence's own justification, since the documented batch metadata (PRV-028) carries no model identity so neither the app nor the user can determine after the fact whether the fallback fired. Verified across four combinations: auto+nova-3+boost on gives both sentences, auto+nova-3+boost off gives base only, auto+nova-2+boost on gives base only, single-language+nova-3+boost on gives the SET-100 string unaffected.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-103|Transcription > button `Test connection`, label becoming `Testing…` while in flight: calls `provider_health(Settings::provider)` and renders `Connected — <message>` or `Failed — <message>`. DISABLED while in flight, and whenever `provider == deepgram` and no key is configured.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-104|Transcription > the `What this provider does` group box renders five claim lines, each prefixed with a ✓ or ✕ mark, driven by the selected provider's `provider_list()` capabilities. It is ALWAYS shown: `Loading provider capabilities…` before the list resolves, and `Cannot describe the selected provider while the provider list is unavailable.` if it fails.|Transcription.svelte|ui/settings|same|same|e2e|done|
|SET-105|Transcription > capability claim `server_side_formatting`: true renders `Applies AI formatting, punctuation and casing on the server.`, false renders `Does not apply server-side AI formatting. You get the provider's raw transcript, cleaned up locally.`|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-106|Transcription > capability claim `accepts_app_context`: true renders `Adapts the output to the app you are dictating into.`, false renders `Does not use the app you are dictating into, so output is not tailored per app.`|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-107|Transcription > capability claim `accepts_screen_context`: true renders `Uses on-screen and focused-field text as context.`, false renders `Does not use screen context, so the Screen context (OCR) setting has no effect.`|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-108|Transcription > capability claim `command_mode`: true renders `Interprets spoken editing commands such as “new paragraph”.`, false renders `Does not interpret spoken editing commands.`|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-109|Transcription > capability claim `vocabulary`: Full renders `Your dictionary — phrases, replacements and snippets — is sent with each dictation and applied by the provider.`, Keyterm renders `Your vocabulary is sent as recognition hints only, up to <max_tokens> terms. Replacements and snippets are applied locally after transcription.`, None renders `Your dictionary is not sent. Replacements and snippets are applied locally after transcription.`|Transcription.svelte|ui/settings|same|same|unit|done|
|SET-112|Account (signed-out) > a `Paste sign-in link` control sits at the bottom of the signed-out account block, so it inherits BOTH mount points — General > Account and the Transcription pane when Wispr Flow is selected (SET-090). It is revealed only when signed OUT and `auth_needs_manual_callback()` returns true. The UI makes NO platform check of its own, deliberately: whether another application owns the `wispr-flow` scheme is a registry fact the webview cannot observe.|n/a (new)|ui/settings|n/a — macOS arbitrates by foreground app, so the command returns false|true only when another app owns the scheme|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|SET-113|Account > Paste sign-in link: if `auth_needs_manual_callback()` is absent or errors, the UI treats it as FALSE and renders nothing, so a backend without the command degrades to no escape hatch rather than confusing noise on every Mac. The command is NOT cached — it reads the registry live on every call, and the UI's effect fires per mount, so uninstalling the conflicting application and reopening Settings picks up the change with no restart. Caching it is the obvious-looking optimisation and would silently strand the user in the exact state the field exists to rescue; changing this requires agreement between the command's owner and the UI, not a unilateral refresh command.|n/a (new)|ui/settings|same|Windows-only; live HKCU/HKCR read per call|unit|done|
|SET-114|Account > Paste sign-in link literals: label `Paste sign-in link`; hint below the label `If signing in does not return to Wispr Lightning, copy the address from your browser and paste it here.`; single-line text field with placeholder `wispr-flow://auth/google/success#...`|n/a (new)|ui/settings|n/a|Windows-only; the reveal is a registry fact|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|SET-115|Account > Paste sign-in link: button `Sign In`, becoming `Signing in…` while in flight, DISABLED while the trimmed field is empty or a submit is in flight.|n/a (new)|ui/settings|n/a|Windows-only; the reveal is a registry fact|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|SET-116|Account > Paste sign-in link validation runs CLIENT-SIDE before any IPC: the value must parse as a URL, its scheme must be `wispr-flow` or `wisprlightning`, and its path must START WITH `auth/` — the slash is required, so a bare `auth` with no trailing segment is rejected inline rather than forwarded. That matches AUT-006's `contains("auth/")` filter exactly, so no paste can produce two different error sentences depending on which side catches it. Both authority forms are accepted — `wispr-flow://auth/...` and `wispr-flow:auth/...` — because non-special schemes put the first segment in the host rather than the pathname. Validation runs against a TRIMMED COPY while the field is submitted verbatim.|n/a (new)|ui/settings|n/a|Windows-only; the reveal is a registry fact|unit|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|SET-117|Account > Paste sign-in link rejection: the inline message `That is not a Wispr sign-in link. Copy the whole address from the browser, starting with wispr-flow://` is shown and NOTHING is sent to the backend. This string is word-for-word the backend's message for the same failure class, adopted deliberately so the user sees ONE sentence whichever side catches it — do not vary them to remove the apparent duplication, the duplication is the point. Verified that wrong scheme, wrong path, bare `auth` and non-URL garbage each reject inline with zero calls reaching the backend.|n/a (new)|ui/settings|n/a|Windows-only; the reveal is a registry fact|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|SET-118|Account > Paste sign-in link success: the field is cleared, `auth_status` is re-read and `session:changed` drives the rest. The pasted value is NEVER logged and does not remain in the DOM — verified that a valid `wispr-flow://auth/google/success#access_token=...` is accepted and the token string is absent from the DOM afterwards. The UI does NOT trim before submitting: `auth_submit_callback` owns the trimming rule, and one owner beats two that can drift. Verified that pasting `  wispr-flow://auth/google/success#access_token=A` submits the leading spaces untouched and still succeeds.|n/a (new)|ui/settings|n/a|Windows-only; the reveal is a registry fact|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|
|SET-119|Account > Paste sign-in link backend failure: the backend's own message renders inline in the same slot the client-side rejection uses, so there is one error location rather than two, and the field is deliberately NOT cleared — the user can fix a truncated paste in place instead of returning to the browser. Clearing on failure would be the tempting symmetry with SET-118 and would be wrong.|n/a (new)|ui/settings|n/a|Windows-only; the reveal is a registry fact|e2e|blocked — Windows-only behavior; we have cross-compile type-checking, not execution on Windows hardware|

## 15. History / Notes / Dictionary windows

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|WIN-001|History renders inside the settings detail pane with no separate window, giving an effective content area of about 640 x 580, resizable.|HistoryWindow.swift|ui/history|same|same|e2e|done|
|WIN-002|History search is a native toolbar search field with the default placeholder `Search`; every keystroke refreshes, using the plain list query when empty and the search query otherwise.|HistoryWindow.swift|ui/history|same|hand-rolled search field in the pane header|e2e|done|
|WIN-003|History empty state is a centered vertical stack with spacing 8: the text-badge-minus glyph at size 36 tertiary, then `No dictations yet` in title3 secondary, with no action button.|HistoryWindow.swift|ui/history|same|substituted glyph|e2e|done|
|WIN-004|The SAME empty state is shown when a search returns nothing — History has no distinct no-results state, unlike Notes and Dictionary.|HistoryWindow.swift|ui/history|same (inconsistency preserved)|same (inconsistency preserved)|e2e|done|
|WIN-005|The history list uses inset style with alternating row backgrounds (zebra striping).|HistoryWindow.swift|ui/history|same|same|e2e|done|
|WIN-006|History entries are bucketed by date into `Today`, `Yesterday`, or a `MMM d` label such as `Mar 4` with no year.|HistoryWindow.swift|ui/history|same|same|unit|done|
|WIN-007|Within a group entries sort newest first, and groups sort by their newest entry with the newest group first.|HistoryWindow.swift|ui/history|same|same|unit|done|
|WIN-008|History section headers render in semibold subheadline, secondary color.|HistoryWindow.swift|ui/history|same|same|e2e|done|
|WIN-009|A history row's metadata line is a spacing-8 horizontal stack in subheadline secondary separated by `·` in quaternary: short-style time such as `3:42 PM`, the app name, the duration formatted `%.1f` with a trailing `s` such as `4.2s`, and `<n> words`.|HistoryWindow.swift|ui/history|same|same|e2e|done|
|WIN-010|A history row has a borderless copy button using the doc-on-doc glyph with tooltip `Copy`.|HistoryWindow.swift|ui/history|same|substituted glyph|e2e|done|
|WIN-011|A history row has a borderless delete button using the trash glyph with tooltip `Delete`.|HistoryWindow.swift|ui/history|same|substituted glyph|e2e|done|
|WIN-012|A history row's body text is `formattedText ?? asrText ?? ""` in body primary, limited to 2 lines with ellipsis truncation.|HistoryWindow.swift|ui/history|same|same|e2e|done|
|WIN-013|History rows have NO context menu — only the two inline buttons.|HistoryWindow.swift|ui/history|same|same|e2e|done|
|WIN-014|A final unlabeled section holds a right-aligned small destructive `Clear All` button, rendered only when the list is non-empty.|HistoryWindow.swift|ui/history|same|same|e2e|done|
|WIN-015|Copy clears the clipboard and writes `formattedText ?? asrText ?? ""` as a plain string, with NO visual confirmation of any kind.|HistoryWindow.swift|ui/history|same|same|e2e|done|
|WIN-016|Delete opens a modal warning alert with message `Delete this entry?`, informative text `This action cannot be undone.` and buttons `Delete` first then `Cancel`; confirming deletes the row and refreshes.|HistoryWindow.swift|ui/history|same (destructive default first, macOS idiom)|in-webview modal with the same wording; button order follows the macOS original for literal parity|e2e|done|
|WIN-017|Clear All opens a modal CRITICAL alert with message `Clear all history?`, informative text `This will delete all transcript entries. This action cannot be undone.` and buttons `Clear All` first then `Cancel`; confirming clears the table and refreshes.|HistoryWindow.swift|ui/history|same|same wording and order|e2e|done|
|WIN-018|Notes renders in the settings detail pane as a zero-spacing vertical stack with a toolbar row padded 8 horizontally and 4 vertically.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-019|Notes search is a custom box — magnifying-glass glyph plus a plain text field — padded 6 with a control-background fill and corner radius 6, placeholder exactly `Search notes…` (U+2026); every keystroke refreshes.|NotesView.swift|ui/notes|same|substituted glyph|e2e|done|
|WIN-020|Notes toolbar has a default-size `New Note` button with a plus glyph.|NotesView.swift|ui/notes|same|substituted glyph|e2e|done|
|WIN-021|Notes empty state with no query is a centered spacing-8 stack: note-text glyph at size 36 tertiary, `No notes yet` in title3 secondary, and a large `Create Note` button.|NotesView.swift|ui/notes|same|substituted glyph|e2e|done|
|WIN-022|Notes with a query and no matches shows a centered spacing-4 stack containing only `No results for "<query>"` in secondary, with the query in literal double quotes.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-023|The notes list order comes straight from the store query with no client-side sorting, using inset style with alternating row backgrounds.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-024|A note row shows the title, or the literal `Untitled` when empty, in medium body weight, with the modified date at short date and short time style such as `3/4/25, 3:42 PM` in caption tertiary on the trailing edge.|NotesView.swift|ui/notes|same|locale-aware short date/time via Intl.DateTimeFormat|e2e|done|
|WIN-025|A note row shows the content preview when non-empty in subheadline secondary, limited to 2 lines.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-026|Right-clicking a note offers `Edit`, a divider, then destructive `Delete`; delete soft-deletes with NO confirmation dialog.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-027|Selecting a note row sets the selection, which immediately opens the editor sheet and clears the selection again, so a single click opens the editor and rows never look persistently selected.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-028|The note editor sheet is 500 x 400 with padding 24 and a spacing-8 vertical stack.|NotesView.swift|ui/notes|same|same at 100% DPI; rem-based under fractional scaling|e2e|done|
|WIN-029|The editor's title field is rounded-border with title3 font and placeholder `Title`.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-030|The editor's body is a text editor in body font with a minimum height of 200 and a 1pt separator border, with NO character limit.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-031|The editor footer has `Cancel` bound to Escape on the leading edge and `Save` bound to Return as the default button on the trailing edge; Save is NEVER disabled, so an empty note can be saved.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-032|Creating a note inserts an empty note, refreshes, looks up the new id and opens the editor on it immediately.|NotesView.swift|ui/notes|same|same|e2e|done|
|WIN-033|Dictionary has an unlabeled segmented picker padded 8 with two segments, `Vocabulary` (tag 0, the default) and `Snippets` (tag 1); the tab selection is view-local and NOT persisted.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-034|Both dictionary tabs bind the SAME search query, so switching tabs carries the filter over, and every refresh recomputes both vocabulary and snippet lists.|DictionaryView.swift|ui/dictionary|same (observable behavior preserved)|same (observable behavior preserved)|e2e|done|
|WIN-035|Vocabulary toolbar has a search box styled like Notes with placeholder `Search vocabulary…` and an `Add Word` button with a plus glyph.|DictionaryView.swift|ui/dictionary|same|substituted glyph|e2e|done|
|WIN-036|Vocabulary empty state with no query is a centered spacing-8 stack: character-book-closed glyph at size 36 tertiary, `No vocabulary words yet` in title3 secondary, and a large `Add Word` button; with a query it is only `No results for "<query>"` in secondary.|DictionaryView.swift|ui/dictionary|same|substituted glyph|e2e|done|
|WIN-037|A vocabulary row shows the phrase in medium body weight with the optional replacement below it in subheadline secondary limited to 1 line.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-038|A vocabulary row shows the optional `source` value as a caption badge with 6 horizontal and 2 vertical padding, accent color at 10% background and corner radius 4.|DictionaryView.swift|ui/dictionary|same|accent shade differs; contrast re-checked|e2e|done|
|WIN-039|A vocabulary row shows the usage count as `<n>x` in caption secondary ONLY when `frequencyUsed > 0` — which never happens today because the column is never incremented.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-040|A vocabulary row shows the modified date at short DATE style only, such as `3/4/25`, in caption tertiary.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-041|Vocabulary rows offer a context menu of `Edit`, a divider, then destructive `Delete`, which soft-deletes with NO confirmation.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-042|The dictionary list order is exactly what the store query returns with no client-side sorting, in inset style with alternating row backgrounds.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-043|The Snippets tab search placeholder is `Search snippets…`.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-044|The Snippets toolbar has THREE items: the search box, `Import CSV` with a square-and-arrow-down glyph, and `Add Snippet` with a plus glyph.|DictionaryView.swift|ui/dictionary|same|substituted glyphs|e2e|done|
|WIN-045|Snippets empty state uses the text-snippet glyph, `No snippets yet`, and a large `Add Snippet` button.|DictionaryView.swift|ui/dictionary|same|substituted glyph|e2e|done|
|WIN-046|A snippet row shows the phrase in medium body weight colored with the accent color and the replacement in subheadline secondary limited to 2 lines, with NO source badge, NO usage count and NO date column.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-047|CSV import opens a native open panel restricted to comma-separated-text with single selection; cancelling does nothing.|DictionaryView.swift|ui/dictionary|same|file dialog with filter CSV/.csv|e2e|done|
|WIN-048|After import a modal alert shows message `Import Complete` and informative text either `Imported <n> entries.` or `Imported <n> entries with <k> errors:` followed by a newline and the first up to five errors joined by newlines.|DictionaryView.swift|ui/dictionary|same|same wording|e2e|done|
|WIN-049|The add-vocabulary sheet is 380 wide with padding 24 and spacing 16: title `Add Vocabulary Word` in semibold title3, a rounded field placeholder `Word or phrase (max 60 chars)` hard-truncated to 60 characters on every change, and a rounded field placeholder `Replacement (optional)` hard-truncated to 200.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-050|The add-vocabulary footer is `Cancel` on Escape and `Add` on Return, with Add disabled while the trimmed phrase is empty; a blank replacement is stored as NULL.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-051|The add-snippet sheet is 420 wide with padding 24 and spacing 16: title `Add Snippet` in semibold title3, a rounded field placeholder `Abbreviation (max 60 chars)` truncated to 60, and an `Expansion` caption above a fixed 100-tall text editor with a 1pt separator border truncated to 4000 characters.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-052|The add-snippet footer is `Cancel` on Escape and `Add` on Return, with Add disabled unless BOTH the trimmed abbreviation and the trimmed expansion are non-empty.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-053|The edit sheet is shared by both tabs at width 420 for snippets and 380 for vocabulary, titled `Edit Snippet` or `Edit Vocabulary Word`, with placeholder `Abbreviation` or `Word or phrase` truncated to 60, and either a 100-tall expansion editor limited to 4000 or a `Replacement (optional)` field limited to 200.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|
|WIN-054|The edit sheet footer is `Cancel` on Escape and `Save` on Return, with Save disabled only while the trimmed phrase is empty — unlike the add sheet, a snippet can be saved with an empty expansion; a blank replacement saves as NULL.|DictionaryView.swift|ui/dictionary|same|same|e2e|done|

## 16. App lifecycle

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|LIF-001|At launch the app calls the accessibility-trust check WITH the prompt option enabled, so a first run raises the Accessibility permission dialog needed for text injection.|AppDelegate.swift|wl-platform::permissions|same|n/a — no equivalent prompt; UIPI/elevation is the analogue|probe|done|
|LIF-002|The trust check logs either `Accessibility: trusted` or a message pointing at System Settings > Privacy & Security > Accessibility.|AppDelegate.swift|wl-platform::permissions|same|n/a|unit|done|
|LIF-003|At launch the app opens the database, constructs the four stores in order and creates all four tables via CREATE TABLE IF NOT EXISTS.|AppDelegate.swift|wl-core::db|same|same|e2e|done|
|LIF-004|At launch the app seeds the default dictionary entries and warms the vocabulary, replacement and snippet caches.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|LIF-005|At launch the mic is pre-warmed only when `keepMicrophoneActive` is true.|AppDelegate.swift|src-tauri::orchestrator|same|same|e2e|done|
|LIF-006|At launch nothing is placed on the overlay: the orchestrator tells it nothing until the first press, which is the first thing it is ever told.|AppDelegate.swift|src-tauri::windows|same|same|e2e|done|
|LIF-025|The overlay window is CONSTRUCTED at launch but not shown, so the first hotkey press pays no window-creation latency.|RecordingOverlay.swift|src-tauri::windows|needs a real Tauri runtime: Overlay::create hardcodes PanelHandle<tauri::Wry>, so no mock runtime can build it|same|e2e|todo — needs AppE2e on a real runtime; Overlay::create hardcodes PanelHandle<tauri::Wry> so the pipeline harness cannot construct it|
|LIF-007|At launch the session is loaded, preferring the app's own credential store and falling back to migrating Lightning's own legacy plaintext `session.json`.|AppDelegate.swift|wl-providers::auth|same|keyring only; Windows has no legacy plaintext file|e2e|done|
|LIF-024|At launch, when neither the credential store nor Lightning's own legacy file yields a session, the Wispr Flow file is consulted and migrated (AUT-039).|AppDelegate.swift|src-tauri::orchestrator|not implemented; no caller reaches the Wispr Flow path|n/a|e2e|todo — startup fallback still absent: load_tokens() reads only app_support_dir()/session.json (session.rs:412). wispr_flow_session_file() now HAS a caller (flow_watcher.rs:76) but that is the watcher-adoption path, not load()'s startup fallback; the log line occurs in no .rs file|
|LIF-008|At launch the Wispr Flow session directory watcher is started from `setup()`, creating the directory first if it is missing, and is torn down at `RunEvent::Exit`.|AppDelegate.swift|src-tauri::flow_watcher|same|n/a|probe|done|
|LIF-009|At launch the deep-link URL handler is registered for the OAuth callback.|AppDelegate.swift|src-tauri::commands|Apple Event get-URL handler for `wispr-flow://`|protocol handler registered only if the scheme is unowned; otherwise the app relies on the paste fallback|e2e|todo — no e2e run exercises this path yet|
|LIF-010|The activation policy at launch follows `showInDock`, so the default false start-up is menu-bar only with no Dock icon.|AppDelegate.swift|src-tauri::tray|same|default start-up is tray only with the taskbar entry skipped|e2e|done|
|LIF-011|System sleep while recording aborts the session completely: state to idle, all timers killed, hotkey listener state reset, the recorder stopped with its packets DISCARDED, the prewarmed connection cancelled, the status bar turned off, the overlay hidden and music resumed.|AppDelegate.swift|wl-core::fsm|NSWorkspace willSleepNotification|WM_POWERBROADCAST PBT_APMSUSPEND|e2e|done|
|LIF-012|At terminate the app closes the history store (a no-op), closes the database handle and cancels the session-file watcher, whose cancel handler closes the descriptor.|AppDelegate.swift|src-tauri::orchestrator|same|same minus the watcher|e2e|todo — no e2e run exercises this path yet|
|LIF-013|An unsent recording persisted before a crash is recovered on the next launch and surfaced with the overlay message `Recovered unsent recording`.|AppDelegate.swift|wl-core::fsm|same|same|e2e|done|
|LIF-014|Only one instance of the app may run; a second launch hands its deep link or activation to the existing instance rather than starting a second tray icon and hotkey hook. CRITICAL CAVEAT ON THIS MACHINE: the Tauri single-instance guard keys on the APP IDENTIFIER, and the user's existing Swift app — `/Applications/Wispr Lightning.app`, running as PID 945 — carries the SAME identifier `com.wisprlightning.app` and already owns its LaunchServices registration. The guard therefore cannot be validated by launching through LaunchServices; it must be tested by launching OUR binary twice by absolute path and asserting the second exits, with the same-identifier third-party app reported as present.|AppDelegate.swift|src-tauri::commands|guard keys on com.wisprlightning.app, which a foreign process already owns on the test machine|explicit single-instance guard plus IPC, required because the protocol callback must reach the running instance|e2e|todo — no e2e run exercises this path yet|
|LIF-015|Launch at login is installed through the OS's user-level login mechanism and its failures are surfaced rather than swallowed.|SettingsWindow.swift|src-tauri::commands|tauri-plugin-autostart with MacosLauncher::LaunchAgent to avoid the TCC automation prompt|tauri-plugin-autostart Run key or Startup shortcut; policy blocks and AV flags must surface in the UI|probe|done|
|LIF-016|Microphone permission denial is surfaced with actionable guidance rather than an opaque start failure, and the guidance must actually work: the Windows deep link is activated from a bounded STA worker, because `ms-settings:` goes through COM and an MTA activation fails silently for exactly those verbs.|AudioRecorder.swift|wl-platform::permissions|TCC prompt on first engine start; denial yields overlay `Mic unavailable`|`E_ACCESSDENIED` surfacing as a cpal BackendError is detected and deep-links `ms-settings:privacy-microphone` from an STA worker|probe|todo — probe layer not yet run for this capability|
|LIF-017|On-disk storage roots resolve per platform and every path used by the app derives from that single root.|DatabaseManager.swift|wl-core::db|`~/Library/Application Support/WisprLightning`|`%APPDATA%\WisprLightning` via the Tauri app-data dir|unit|done|
|LIF-018|Bundled resources — the sidebar icon PNG and the sound packs — are resolved through the platform resource mechanism, never a hardcoded absolute path.|SoundManager.swift|src-tauri::commands|bundle resource URL lookup|Tauri resource resolver relative to the exe|e2e|todo — no e2e run exercises this path yet|
|LIF-019|Raw AppKit and raw Win32 objects are MAIN-THREAD / correct-apartment only, and AppKit traps rather than misbehaving: every panel, window, tray and menu operation reaching a raw platform object is routed appropriately, never called directly from a tokio worker. Tauri hops the thread for its own API, but a crate wrapping the same objects does not inherit that guarantee. Audited exhaustively, not spot-checked, and it found two further violations beyond the original overlay crash: `NSAppleScript` executed on worker threads (macOS), and `ShellExecuteW` called from the implicit MTA (Windows). Enforcement is now STRUCTURAL rather than by convention — `macos/main_thread.rs` hands the closure an `objc2::MainThreadMarker` and both `run_script` and the `UCKeyTranslate` layout builder DEMAND one, so neither is reachable without proof of the main thread; all Windows apartment policy lives in `windows/mod.rs`.|n/a (new)|src-tauri::windows|every AppKit/CoreGraphics/CoreFoundation/Vision/Carbon/AX entry point audited; MainThreadMarker enforced at the type level|bounded STA worker for shell activation; apartment policy centralised in windows/mod.rs|e2e|done — Windows half: source audit and cross-compilation only; no Windows execution was performed|
|LIF-020|The main-thread affinity failure mode is a crash, not a misbehavior, and cannot be caught by a unit test — it needs a running app driving the overlay from a worker thread, which is why the e2e smoke exercises exactly that path.|n/a (new)|src-tauri::windows|observed: SIGTRAP 226 ms after launch on the first overlay show|same class for raw Win32 from the wrong thread|e2e|done|
|LIF-021|`src-tauri/capabilities/` must exist and must declare a permission for every plugin command and for `listen`; Tauri v2 blocks them by default and an undeclared command fails SILENTLY at runtime rather than at compile time.|n/a (new)|src-tauri::commands|same|same|e2e|todo — the webview working proves the happy path, not that a missing permission is caught|
|LIF-022|The shipping macOS artifact is a signed `.app` bundle whose identity the OS depends on: `Identifier=com.wisprlightning.app`, hardened runtime enabled (`flags=0x10000(runtime)`), entitlements `com.apple.security.device.audio-input` and `com.apple.security.automation.apple-events`, `LSUIElement=true`, `NSMicrophoneUsageDescription` present, and `CFBundleURLSchemes=["wispr-flow","wisprlightning"]`. This is not packaging trivia: TCC keys grants on the SIGNATURE, so ad-hoc re-signing during development resets every permission the user granted, and a bare unbundled binary has no bundle identifier at all — which is why it cannot be activated (SET-009) and why CoreAudio hands it digital silence instead of microphone input.|n/a (new)|src-tauri::commands|verified by codesign: 13 MB bundle, 9.9 MB binary, resources bundled; NOT notarized, which matters only for distribution|n/a — no equivalent signing/TCC model|e2e|done|
|LIF-023|The port DELIBERATELY reuses the Swift app's bundle identifier `com.wisprlightning.app`, and this must not be changed. TCC keys grants on the identifier, so reusing it carries an upgrading user's Accessibility, Input Monitoring, Microphone and Screen Recording grants across silently, along with their settings and their `lightning.db`. Changing it revokes all four: the app launches, looks completely fine, and never responds to the hotkey until the user re-grants permissions they already granted once — which is precisely the silent-failure mode LOG-012 exists to describe, and far worse than any LaunchServices ambiguity. The cost is that installing the new app ALONGSIDE the old one rather than replacing it leaves two bundles sharing one identifier, which LaunchServices cannot disambiguate (LIF-014, AUT-004). A real upgrade replaces the bundle and the ambiguity never arises, so the install instructions MUST say replace, not add.|n/a (new)|src-tauri::commands|identifier reuse is the mechanism for TCC and data continuity|n/a — no equivalent signing/TCC model|manual: install the new bundle over the old one, confirm only one `Wispr Lightning.app` remains in /Applications, then launch and verify the hotkey works with no new permission prompts|todo — upgrade-in-place path not yet exercised|

## 17. Logging & diagnostics

|ID|Behavior|Swift source|Owner (crate/module)|macOS|Windows|Verify|Status|
|---|---|---|---|---|---|---|---|
|LOG-001|All diagnostic output goes to the OS system log with the `Wispr Lightning: ` prefix on user-facing lines.|AppDelegate.swift|src-tauri::orchestrator|unified system log|structured file log under %LOCALAPPDATA% plus stderr|unit|done|
|LOG-002|Verbose logging is off by default and, when enabled, writes full server requests and responses to `~/Library/Logs/WisprLightning.log`.|Settings.swift|src-tauri::orchestrator|same|log file under %LOCALAPPDATA%\WisprLightning\Logs|unit|done|
|LOG-003|With verbose logging on, the full polish request body is logged as `Polish request body: <json>`.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|LOG-004|With verbose logging on, the raw polish response body is logged as `Polish response: <body>`.|PolishService.swift|wl-providers::polish|same|same|unit|done|
|LOG-005|With verbose logging on, each audio chunk logs `WS sending chunk <offset>..<<end> of <total> (<n> bytes, final=<bool>)`.|TranscriptionClient.swift|wl-providers::wispr|same|same|unit|done|
|LOG-006|With verbose logging on, each received frame logs `WS received: <first 500 chars>` — truncated, so a long frame is never dumped in full.|TranscriptionClient.swift|wl-providers::wispr|same|same|unit|done|
|LOG-007|With verbose logging on, a token refresh logs the first 300 characters of the response body.|Session.swift|wl-providers::auth|same|same|unit|done|
|LOG-008|Every hotkey evaluation logs one line carrying path label, keycode, pressed, onScreen, localHID, paused and keyDown, regardless of whether it triggered.|HotkeyListener.swift|wl-platform::macos::hotkey|same|same with path label `ll-hook`|unit|done|
|LOG-009|Database open success logs `Wispr Lightning: Database opened at %@` and failure logs `Wispr Lightning: Failed to open database at %@`.|DatabaseManager.swift|wl-core::db|same|same|unit|done|
|LOG-010|The legacy database rename logs `Wispr Lightning: Migrated history.db → lightning.db` using a real U+2192 arrow, not `->`.|DatabaseManager.swift|wl-core::db|same|n/a|unit|done|
|LOG-011|Engine start logs `Audio engine started (input: %@, rate: %.0f Hz)` and stop logs `Recording stopped — %d packets (%.1fs)`.|AudioRecorder.swift|wl-platform::audio|same|same|unit|done|
|LOG-012|There is NO diagnostic anywhere for denied Input Monitoring — the app looks alive and simply never triggers; the port surfaces permission status through the platform Permissions trait instead.|HotkeyListener.swift|wl-platform::permissions|DEVIATION: permission status is queryable and surfaced|DEVIATION: UIPI and hook-removal states are detected and surfaced|probe|todo — probe layer not yet run for this capability|
|LOG-013|The platform probe binary exercises every platform trait on the current OS and prints pass or fail per capability: enumerate and record 1 s, capture a hotkey for 5 s, inject into a scratch field, read focused text, OCR, media pause and resume, and permission status.|n/a (new)|wl-platform::probe|run on macOS, output pasted into this matrix as evidence|run on Windows, output pasted into this matrix as evidence|probe|todo — probe layer not yet run for this capability|

## Open questions

Genuinely ambiguous items the three specs do not settle for the Windows port. Each must be resolved
before the row(s) that depend on it can close.

**Q1. RESOLVED — `client_platform` stays `"darwin"` on both platforms.**
Not a placeholder awaiting better information; it is the terminal state. `"darwin"` is the only
value KNOWN to be accepted, because it is the only value the shipping client has ever sent. Any
alternative is a guess, and a guess is the one thing that can produce a hard rejection. The
asymmetry settles it: being wrong about `"darwin"` costs at most server-side analytics attribution,
while being wrong about a guessed value costs the entire transcription path — so the pin stays
lowest-risk even after the truth is known.

Observation is also not schedulable. It needs a live Wispr Flow Windows client, a real account and
traffic interception against a private API; no phase of this port creates any of those, so
scheduling it would move the same unknown to a later date with more ceremony. Recorded as PORT_PLAN
section 8 debt item 3, trigger "observing what the real Windows client sends, or a rejection in the
wild". It is a single `const` in `wl-providers/src/wispr.rs`, so the change is one line if evidence
appears. An open list should only hold things somebody can actually do.

**Q2. What value should `context.app.bundle_id` carry on Windows?**
The server keys personalization off `bundle_id` (WSS-007). Windows has no bundle identifiers.
PORT_PLAN section 4 proposes the exe basename, but a personalization profile built up under
`com.slack.Slack` will not match `slack.exe`, so a Windows user starts with an empty profile.
Candidates: exe basename, AppUserModelID, or a synthetic mapping table that emits the macOS bundle
id for known apps. Affects CTX-003 and WSS-007.

**Q3. Does the Windows build of Wispr Flow write a session file this app can read?**
The macOS migration path reads `~/Library/Application Support/Wispr Flow/session.json` and parses
the first key containing `auth-token` (AUT-019, AUT-020). The Windows storage location
(`%APPDATA%\Wispr Flow\session.json` is an inference) and format are unverified. Current
resolution is to mark the whole feature macOS-only (AUT-019, AUT-031, AUT-032, LIF-008). Reopen only
if the Windows format is confirmed by inspection, not by assumption.

**Q4. Will the Supabase project accept a loopback redirect URL?**
The Windows OAuth resolution is `http://127.0.0.1:<ephemeral>/auth/callback` (AUT-001), which
requires the redirect URL to be allow-listed in the Supabase project configuration. If it cannot be
added, the fallback is registering `wisprlightning://` and accepting the loss of the Wispr Flow
handoff. Affects AUT-001, AUT-004, AUT-005, LIF-009.

**Q5. Should `client_version` stay `"1.4.549"` on Windows?**
It is a macOS Wispr Flow version string. Changing it risks the same class of silent server-side
behavior change as Q1. Default is to keep it. Affects WSS-020.

**Q6. RESOLVED — the tray menu is fully specified.**
Decision from Main: in order, the last-transcription preview (or the disabled item
`No recent dictation`) which copies on click; separator; the `Input Device` submenu (`System
Default`, separator, one checkable item per device); `Pause hotkey` / `Resume hotkey` with the title
flipping and the item checked while paused; checkable `Natural Mode`; `Settings` on Cmd+comma;
separator; `Quit Wispr Lightning`. The icon is `Resources/WisprFlowIcon.png` at 18pt and is NOT a
template image. Section 12 grew from 9 rows to 20 (TRY-008, TRY-010 through TRY-020) and no longer
contains a manual row. **Consequence, now tracked as Q21:** the enumerated menu has no today's-stats
item, so `HistoryStore.todayStats()` is left with no named consumer.

**Q7. Modal button order on Windows.**
The delete and clear-all alerts put the destructive action FIRST (WIN-016, WIN-017), which is the
macOS idiom and the reverse of the Windows convention. Matching macOS exactly will feel wrong on
Windows; deviating breaks literal parity. Currently the rows specify macOS order on both platforms.
Pick one and record it as a deviation if it changes.

**Q8. Left/right modifier aliasing.**
macOS cannot distinguish which Control is still held because both share one flag bit (HTK-009).
Windows can. "Press L-Ctrl, press R-Ctrl, release L-Ctrl" therefore behaves differently, and the
Windows behavior is strictly more correct. The rows currently say the quirk is not reproduced.
Confirm that a user relying on the macOS behavior is not broken by this.

**Q9. Should Windows reject injected keyboard events?**
`LLKHF_INJECTED` is the analogue of the macOS PID-zero check (HTK-011), but AutoHotkey, keyboard
remappers and some KVMs set it. Rejecting injected events would break users who work fine on macOS,
where those tools present as real HID input. Undecided.

**Q10. Windows OCR when the language pack is absent.**
`Windows.Media.Ocr` requires an installed language pack (CTX-016). With none installed the OCR path
yields nothing and the user gets silently worse formatting, exactly like a denied Screen Recording
grant on macOS (CTX-019). Decide whether to detect and surface this or to reproduce the silent
degradation.

**Q11. Does the `Show in Dock` row survive on Windows?**
"Dock" is meaningless there, and taskbar visibility is not the same concept (SET-076, TRY-009).
Relabelling to "Show in taskbar" is currently assumed; hiding the row entirely would change the
System tab layout.

**Q12. Which version string does the About line display?**
The System tab hardcodes `Wispr Lightning v1.0.0` while `clientVersion` is `1.4.549` (SET-082).
Literal parity preserves the mismatch. Decide whether the port shows the real build version.

**Q13. Do we ship the soft-deleted-phrase trap?**
`UNIQUE(phrase, team_dictionary_id)` plus `INSERT OR IGNORE` means a soft-deleted dictionary phrase
can never be re-added through the UI (DB-043). DB-043 currently says the bug is preserved. It is not
on the DV1-DV8 list, so preserving it is the default — confirm that is intentional.

**Q14. Migrating the mic selection across platforms.**
CoreAudio UIDs and WASAPI endpoint ids are different namespaces (SET-033, TRY-021). A settings file
carried from macOS to Windows holds a `coreaudio:<uid>` in `mic_device_id` that matches no endpoint,
so it must reset to System Default; the stored `mic_device_name` is shown as a stale hint only and
is never used to re-match the device (see Q22). Confirm whether the reset is silent or surfaced.

**Q15. What happens to a user bound to `Fn` (keycode 63)?**
Windows has no Fn virtual key (HTK-002, HTK-044). Their hotkey cannot migrate. Decide between
falling back to Left Control and leaving the app hotkey-less with a visible warning.

**Q16. Clipboard formats that cannot round-trip.**
Windows delayed-rendering formats such as `CF_HDROP` and owner-rendered `CF_BITMAP` cannot be saved
and restored byte-for-byte (INJ-007, INJ-015). Some clipboard contents will be lost by every
dictation. Decide whether to snapshot only text-like formats, or to accept the loss.

**Q17. Which icon set replaces SF Symbols?**
Twenty-eight distinct SF Symbols are used and none are licensable off Apple platforms (SET-013 and
most of section 15). Row heights and optical weights shift with any replacement set, so the choice
changes the visual parity baseline for every UI row.

**Q18. RESOLVED — provider selection is global.**
Decision from Main: `Settings::provider` is the single source of truth. This app has no profile
concept and none is being invented; "per-profile" in PORT_PLAN section 3.2 was loose wording and the
plan document is being corrected. PRV-021 now states the global contract explicitly.

**Q19. Overlay visibility across Windows virtual desktops.**
macOS gets all-Spaces visibility for free (OVL-006); Win32 has no equivalent, so the overlay is
confined to the desktop it was created on unless re-created on desktop switch. Decide between
re-creation and accepting single-desktop behavior. OVL-006 is `manual` until this is settled.

**Q20. Where does the Windows verbose log live, and what does the settings description say?**
SET-079's description string names `~/Library/Logs/WisprLightning.log` verbatim. The Windows text
must differ, which means the settings copy is no longer a single verbatim string shared by both
platforms.

**Q21. RESOLVED — `todayStats()` and `playPaste()` are both dead code.**
Main grepped the Swift tree: `todayStats()` appears exactly once (its own definition at
HistoryStore.swift:99) and `playPaste()` exactly once (SoundManager.swift:94). Zero call sites for
either, same class as `ToastNotification`. Recorded in PORT_PLAN section 2 as **DV9** (todayStats not
ported) and **DV10** (playPaste and `paste.wav` not ported). DB-028 and SND-021 are RETIRED IN PLACE
rather than deleted, carrying `DEVIATION DV9` / `DEVIATION DV10` and `Verify = n/a`, so the decision
stays visible to anyone who later wonders where the daily counter went. SND-014 and SND-015 were
amended so they no longer claim the port loads three sounds.

**Q22. RESOLVED — both fields are persisted; only *resolving* by name is forbidden.**
Confirmed by Main, and it is already what `crates/wl-core/src/settings.rs` carries:

```rust
mic_device_id:   Option<String>,  // "coreaudio:<uid>" / "wasapi:<endpoint>"; None = system default
mic_device_name: Option<String>,  // display label only
```

`mic_device_id` is the sole resolution key and the only value ever compared against enumerated
devices. `mic_device_name` has exactly two jobs: labelling the current selection in the settings
picker and the tray submenu before enumeration completes, and naming the device in the
unavailable-mic log line (`Requested mic 'Yeti Nano' not available, using system default`). Nothing
ever matches on it, so two identical USB mics sharing a name is harmless. The earlier wording
conflated storing the name with resolving by it.

The alternative — drop the field and derive labels from live enumeration — was rejected because it
makes the failure case strictly worse: when the stored device is absent there is nothing to
enumerate, so the UI could only say `unknown device` instead of naming the missing mic. A stale
string costs nothing and produces a materially better message. AUD-011, AUD-013, AUD-016, SET-033,
SET-034, TRY-014, TRY-015 and TRY-021 all now state explicitly that the name is never a resolution
key; AUD-013 previously claimed the name was not persisted at all and has been corrected.

**Q23. RESOLVED — split the case, and DV11 extends to `Session::refresh`.**
Part 1, from Main: 402 and 429 are different failures and must not share a variant.
`QuotaExceeded` takes 402 only, is NOT retryable, and reads `Out of credits — check your <provider>
account`; `RateLimited` takes 429 only, IS retryable, and reads `Rate limited — try again in a
moment`. 402 never clears by retrying, so retrying it is the exact defect DV11 exists to fix; 429
always clears with backoff, so refusing to retry it strands a recording that would have succeeded.
Encoded as POL-038 (402) and POL-039 (429).

Part 2, from Main: yes, DV11 is widened to `Session::refresh`, same defect with a different blast
radius. Today every refresh failure collapses to `authFailed`, so a transient Supabase 500 or a
flaky network throws the user at a sign-in screen and discards a recording a retry would have
completed. 400/401/403 means the refresh token is genuinely dead and is the only path that asks the
user to sign in again; 408, any 5xx, a timeout or a transport error is retryable and handled by the
existing pipeline retry, invisibly. Encoded as AUT-033, AUT-034 and AUT-035.

**Q24. RESOLVED — the Transcription pane is fully rowed.**
FrontendSettings supplied the inventory verbatim from `Transcription.svelte`. Section 14 gained
SET-088 through SET-109: pane chrome, the provider segmented control, the re-mounted account block,
the write-only API key field, the key-status text, Save Key, Clear, Model, the two Deepgram
switches, the language-mapping notice and its four mutually exclusive variants, Test connection, the
capability panel with its loading and failure states, and one row per capability claim including all
three `vocabulary` forms. No literal was invented. Two consequences beyond section 14: the
`auto` handling is provider behavior, not just copy, so it is rowed as PRV-026 (`detect_language=true`
with `language` omitted — the literal `auto` is never sent) and PRV-027 (the none/one/many mapping
that SET-098 mirrors and must not drift from).

**Q25. RESOLVED — adopt the UI's three-part condition, and add a second provider warning.**
Main's ruling. The auto-detect warning takes the UI's condition: boost AND Nova-3 family AND Detect
(PRV-029). Warning a user on nova-2 that auto-detect may drop them off Nova-3 is incoherent — they
were never on it, and boost was already doing nothing.

That last clause was the real finding: `deepgramKeytermBoost = true` on a non-Nova-3 model is its
own silent no-op, reachable because SET-096 greys the switch without writing `false`, so the control
renders disabled and still checked. The provider therefore logs TWO warnings, not one — PRV-029 for
the auto-detect case and PRV-030 for `boost AND NOT Nova-3 family`, reading
`keyterm boosting is ignored: <model> is not a Nova-3 family model.` They are mutually exclusive by
construction, so no configuration produces both, and every configuration in which boost silently
does nothing now produces exactly one log line.

The UI stays exactly as shipped: one sentence, auto-detect only (SET-110). The non-Nova-3 case is
already communicated by the greyed switch, so a second sentence would be redundant on screen while
the log still needs it for a support report. SET-096 records the disabled-and-checked rendering and
why neither alternative (render off, or clear the value on model change) was taken.

## Evidence log

Reports handed in by implementing agents ahead of Main's Phase 6 reconciliation. **No `Status` value
has been changed on the strength of these** — the column contract says a row closes only when the
artifact named in `Verify` runs green and is linked from the closing commit, and reconciliation is
Main's pass, not mine. Recorded here so the evidence is not lost in IRC scrollback.

**Section 13, `ui/overlay` half — FrontendApp, browser-verified against fixture and rejecting
backends.** OVL-004, 005, 008-016, 019-023, 026-038, 042 and the height half of OVL-017 implemented.
Widths measured with `getBoundingClientRect`: 120, 120, 145, 175, 180, 300, and 200 once the elapsed
label appears; height 36 in all seven states including Hidden. Pulse computed style is
`0.6s ease-in-out infinite alternate`. The Save one-shot to `Saved`+disabled and its reset on the
next Recording were both exercised. Warning levels 1 and 2 confirmed to change only the tint.
`prefers-reduced-motion` pins the dot to opacity 1 rather than freezing it at 0.3.
OVL-017 and OVL-024 are the two split-ownership rows; the `src-tauri::windows` halves are
TauriShell's to confirm.

**Section 15, all 54 rows — FrontendApp.** Specifically exercised beyond the obvious: WIN-004's
preserved inconsistency, with History showing `No dictations yet` for a no-results search while
Notes and Dictionary show `No results for "<query>"`, all three checked side by side; WIN-032's
create-then-open-editor with the `notes_update` payload asserted; WIN-028's 500x400 at padding 24
and spacing 8 measured; the 380 and 420 sheet widths and the 60/200/4000 caps of WIN-049, WIN-051
and WIN-053 measured; the differing Add and Save gating of WIN-052 against WIN-054; WIN-048's
multi-line error report; and pagination 50 -> scroll -> 100 -> Load More -> 130 with the control
disappearing once exhausted. Two implementation notes folded into the row text: the
"hard-truncated on every change" limits are implemented as `maxlength`, which also caps paste, and
WIN-047 required `tauri-plugin-dialog`, added mid-flight with Main's approval.

**Section 14 — FrontendSettings.** The four accepted deviations (SET-012, SET-046, SET-081,
SET-084) and the System Permissions block (SET-085 to SET-087) are implemented and smoke-tested.

**src-tauri shell slice — TauriShell.** 106 tests green on macOS, `cargo xwin check --lib --tests
--target x86_64-pc-windows-msvc` clean, zero warnings on both targets.

*Live-verified by driving the running binary through the macOS accessibility API — e2e evidence, not
unit assertions.* OVL-043: Safari frontmost, overlay shown, Safari still frontmost, app
`frontmost=false`, asserted directly rather than inferred from a successful injection.
OVL-017 and OVL-026: live window measured 300x36 at position 585,796 on a 1470x956 display, where
585 is exactly (1470-300)/2 and 796+36+50 equals 882, the work-area bottom edge. TRY-010, 012, 013,
016, 017, 018, 019, 020: real tray menu read back in order — `No recent dictation` (disabled),
separator, `Input Device`, `Pause hotkey`, `Natural Mode`, `Settings`, separator,
`Quit Wispr Lightning`. TRY-014: submenu read back as `System Default` bearing the check, separator,
then both real devices. SET-001, 004, 007, 008: settings window opened from the tray at 860x581
titled `Wispr Lightning Settings`, closed to hidden rather than destroyed, reopened at the identical
position 305,168. TRY-001 and LIF-010: process reports `visible=false` with no Dock icon.

*Unit-covered.* TRY-021 has a dedicated test: a macOS `coreaudio:` id against a WASAPI device list
must move the check mark to System Default and never leave every item unchecked. Overlay widths
cover all seven states in both elapsed-visible variants (OVL-020 to OVL-026, OVL-032), height 36
everywhere, the centring invariant across widths, and an offset work area. Deep-link parsing covers
query form, fragment form, query-wins-on-conflict, percent decoding, both schemes, the
authority-less `scheme:auth/...` form, non-auth path, foreign scheme, and empty-parameter and
malformed URLs. Overlay event payloads are pinned to the frozen wire shape including
`{"Retrying":{"attempt":1,"of":3}}`.

*Explicitly NOT to be closed on this report.* **SET-009** activation is implemented but provably
without effect for an unbundled dev binary — no bundle identifier, and macOS refuses to activate
such a process — so it needs a bundled-.app re-check, which Main is covering in the bundled smoke.
**OVL-004** is not satisfied but deviated: the window is built shadow-disabled and the frontend
draws the shadow in CSS.

**Section 5.4 platform probe, run on real macOS hardware — Main.** Permissions: Microphone
NotDetermined, Accessibility / InputMonitoring / ScreenRecording all Granted. Frontmost:
`name="Safari" bundle_id="com.apple.Safari" kind=other`. Focused text: 0 lines in 425µs (empty
field, call path exercised). OCR: 50 lines at the cap in 148.6 ms from the real Safari window. Mics:
2 devices with stable CoreAudio UIDs (`coreaudio:BuiltInMicrophoneDevice`,
`coreaudio:05EA9D76-…`), system default correctly marked. Record 3 s: prewarm 73.2 ms, start 1.58µs,
stop 7.2 ms, **exactly 75 packets = 3.00 s with zero sample loss**, and `faults: [SilentInput]` at
peak 0.0000 — correct, because Microphone is NotDetermined on an unsigned dev binary so CoreAudio
returns digital silence. That is precisely the condition AUD-037 exists for (the Windows
privacy-toggle case), caught in the wild rather than simulated. Cues: 4 packs discovered, overlap
confirmed with start and stop 40 ms apart, mute respected. Music: `pause()` returned false in
124.9 ms with nothing playing and `resume()` started nothing, so the "never resurrect a player the
user stopped" invariant holds. Inject (Paste): `Paste path: 你好 🎤 ok.` landed byte-exact in
TextEdit with verified=true in 134.3 ms and the clipboard restored — CJK and emoji both survive.
Inject (Natural): 31 chars at 4.0 cps in 9.85 s against a ~9.5 s prediction with jitter and hold.
Selection: round-tripped `"Natural mode, typed: it's fine!"` back out via `copy_selection`.
Lifecycle: sleep observer installed, `launch_at_login()` false, `set_launch_at_login` correctly
deferred to the autostart plugin.

*Deliberately NOT closed by this run, with the reason on each row:* AUD-012 (recorded on the system
default, so binding to a chosen device is unexercised), CTX-019 (ScreenRecording was Granted, so the
denial path was never taken), INJ-026 and INJ-027 (Return and Tab key paths unexercised — "punctuation
correct" does not cover them), LIF-019 and LIF-020 (one fixed instance is not the claim that EVERY
surface is routed; awaiting audit statements), LIF-021 (a working webview proves the happy path, not
that a missing permission is caught, which is the entire danger of that row).

**Main-thread and apartment audits — MacThreadAudit and WinThreadAudit.** Both found genuine bugs,
which is why LIF-019 was correctly refused on the strength of the single overlay fix: there were two
more.

*macOS.* Exhaustive audit of every AppKit, CoreGraphics, CoreFoundation, Vision, Carbon and AX entry
point in `wl-platform/src/macos/**` and `src-tauri/src/{overlay,tray,windows,state,commands}.rs`. One
real violation: **`NSAppleScript` was constructed and executed on worker threads** in `media.rs`.
Apple's Thread Safety Summary lists it as the ONLY entry under "Main Thread Only Classes", and since
the December 2025 XProtect update a script object first created off the main thread hangs the
process. Fixed structurally: `macos/main_thread.rs` hands the closure an `objc2::MainThreadMarker`,
and both `run_script` and the `UCKeyTranslate` layout builder now demand one, so neither is
reachable without proof of the main thread. Evidence method worth recording: main-actor isolation
was read off the compiler by building a `nonisolated` Swift 6 probe against the macOS 26.5 SDK,
rather than trusting prose — which also corrected a `mod.rs` doc comment that had asserted
`NSAppleScript` was thread-safe.

*Windows — source audit and cross-compilation only; no Windows execution was performed.* One real
violation: **`ShellExecuteW` called from the process's implicit MTA.** `open_settings` is reached
from an async Tauri command on a tokio worker, and `ensure_mta` puts those workers in the MTA on
every dictation; the shell publishes no marshalling info, so a shell call from an MTA fails for
exactly the verbs that involve a COM object, which is why it presents as intermittent.
`ms-settings:` activation goes through COM. Consequence: the microphone settings page would silently
fail to open, removing the only remediation a user has after being told microphone access is Denied.
Fixed with a bounded STA worker, with all apartment policy now centralised in `windows/mod.rs`.
PowerShell carried the identical bug and fixed it the identical way.

*Post-audit state, verified by Main:* 512 tests passing, 0 clippy warnings, `cargo fmt` clean,
`cargo xwin check --workspace --all-targets` clean, app relaunched and the focus invariant still
holds. Incidental win: `pause()` fell from 124.9 ms to **6.3 ms**, because the fix short-circuits on
the NSWorkspace running-player check before any AppleScript hop.

**HTK-049 ui/settings half — FrontendSettings.** Production bundle via `vite preview`, headless
Chromium, IPC bridge mocked, `hotkey_capture_end` returning null forever so the capture never
yields a chord; armed, allowed to poll, cancelled with the second click. Before: button
`Add Hotkey`, one keycap `Left Control`, 0 saves, 0 remove buttons. While capturing: button
`Press a key…`, keycap unchanged, 0 saves. After cancel: button reverted, keycap unchanged, 0 saves,
still 0 remove buttons. After a further 1.2 s — well past the 250 ms save debounce, so a late write
would have surfaced — still 0 saves. Total `settings_save` invocations across the entire run: **0**.
Capture calls: 6 begin, 7 end, the extra end being the deliberate final disarm so the backend is not
left suppressing the real hotkey handler. PipelineE2e closed the backend-adjacent half and correctly
declined to claim the Svelte half, since `ui/` has no test runner and adding one was not their call.

**Signed macOS bundle available — Main.** `target/main/release/bundle/macos/Wispr Lightning.app`,
verified by codesign inspection: `Identifier=com.wisprlightning.app`, `Authority=Claude Voice Dev`,
`flags=0x10000(runtime)`, entitlements `com.apple.security.device.audio-input` and
`com.apple.security.automation.apple-events`, `LSUIElement=true`, `NSMicrophoneUsageDescription`
present, `CFBundleURLSchemes=["wispr-flow","wisprlightning"]`, resources bundled. 13 MB total,
9.9 MB binary. Not notarized — irrelevant to local execution, relevant only to distribution.

The bundle CLOSES only LIF-022, its own identity facts. It ENABLES, but does not by itself close,
LIF-014, LIF-018, AUT-005, AUT-016 and the SET-009 activation re-check — all of which need a run
from inside the bundle and remain open rather than being inferred from the artifact existing.

**Measurement hazard on this machine — identity collision, reported by Main after hitting it.** The
Swift app this port replaces is installed at `/Applications/Wispr Lightning.app`, carries the SAME
bundle identifier `com.wisprlightning.app` — deliberately, see LIF-023 — and has been running as PID
945 for over five days. Because we build to `target/` while theirs sits in `/Applications`, two
bundles share one identifier at once; a real upgrade replaces the bundle and this does not arise.
Consequences, all confirmed here read-only: `open -a "…/Wispr Lightning.app"` is AMBIGUOUS and
LaunchServices resolved it to the `/Applications` copy, so a launch can silently measure the wrong
process; `pgrep -f "Wispr Lightning"` matches BOTH, because the port's bundle directory carries the
same display name; and any System Events lookup BY NAME finds the Swift app. An RSS figure initially
attributed to the port was retracted for exactly this reason — it was the Swift app's.

*Correct method, verified here:* launch by absolute binary path, never `open -a`; identify with
`pgrep -f "target/main/release/bundle/macos"` and confirm via `ps -o command= -p <pid>`. The cheapest
discriminator is the executable name — theirs is `MacOS/WisprLightning`, the port's is
`MacOS/wispr-lightning`. Observed simultaneously: PID 945 at 05-08:19:32 on the `/Applications` path,
PID 30091 at 00:51 on the bundle path. **PID 945 is the user's real app doing real work and must not
be killed.**

**False-green correction — AppE2e, verified independently here by source inspection.** Two rows
marked `done` described behaviour that does not exist, in both cases because a single row carried
TWO clauses and only the first was implemented. AUT-019 and LIF-007 each claimed a Wispr Flow
session fallback. Reality in `crates/wl-providers/src/session.rs:398-424`: `load_tokens()` reads the
credential store and on a miss calls `migrate_legacy_file(app_support_dir().join("session.json"))` —
Lightning's OWN legacy plaintext file, not the commercial app's. Confirmed here:
`wl_core::paths::wispr_flow_session_file()` (paths.rs:66) has exactly one occurrence in the
workspace, its own definition; the string `Migrated session from Wispr Flow` occurs in zero `.rs`
files; and `notify`/FSEvents/kqueue occur zero times across `crates/` and `src-tauri/`.

Both rows were SPLIT rather than reopened, so the implemented half keeps its evidence: AUT-019 and
LIF-007 now cover only the legacy-plaintext migration and stay `done`; AUT-039 and LIF-024 carry the
Wispr Flow fallback and are `todo`. LIF-008, AUT-031 and AUT-032 keep their `todo` but their reason
now states the feature is ABSENT rather than merely unproven — `Session::adopt()` exists and is
unit-tested but has no production caller, and `FlowWatcher` is commissioned to implement it. That
distinction matters at reconciliation: "no evidence yet" and "no code yet" are different amounts of
remaining work.

*Systemic note.* Both false greens came from the blanket rule "Verify `unit` plus a green crate means
`done`", applied to rows whose Behavior contained two independent claims. A unit test can cover one
clause and leave the other unwritten while the row still reads as satisfied. The split-rather-than-
collapse discipline used elsewhere in this matrix is exactly what prevents it; these two rows
predated its consistent application, and any remaining multi-clause `done` row is the same exposure.

**Watcher landed mid-audit — caught by a compile error, not by a report.** Minutes after the
false-green correction above was written, `FlowWatcher` landed `src-tauri/src/flow_watcher.rs` (347
lines, FSEvents, start and stop wired at `lib.rs:146` and `lib.rs:303`), which made the reason text
on LIF-008, AUT-031 and AUT-032 factually wrong — they asserted no watcher existed anywhere in the
workspace, which had been true when verified and was no longer. Re-checked and split three ways
rather than blanket-updated: LIF-008 and AUT-031 now read "landed, not yet proven"; AUT-032 stays
absent because `Session::adopt()` still has ZERO callers, so nothing adopts a session on a watcher
event; and AUT-039 / LIF-024 stay absent but with a corrected reason, because
`wispr_flow_session_file()` now HAS a caller at `flow_watcher.rs:76` — the watcher-adoption path, not
`load()`'s startup fallback, which at `session.rs:412` still reads only
`app_support_dir()/session.json`. The log line still occurs in no `.rs` file.

*Worth naming:* this drift was invisible from the matrix side. It surfaced only because PipelineE2e
reported an unrelated E0425 in `lib.rs`, which prompted a re-read of a file whose facts the matrix
had already recorded as settled. A matrix that cites source is only as fresh as its last re-check.

**Self-audit: the summary was making a false claim about the document.** The Status summary asserted
that every remaining `todo` and `blocked` row carries its reason inline. It did not — 42 of the 68
open rows had an empty reason, so the assertion was true of the 26 rows that had been individually
reasoned and false of the rest. Rather than weaken the sentence to match, every open row now carries
an accurate reason derived from its own verification layer: `probe layer not yet run for this
capability` (14), `no e2e run exercises this path yet` (26), `manual step not yet performed` (2), and
the individually-reasoned rows unchanged. AUD-019 got a specific one, since
`CaptureFault::DevicesChanged` handling has now landed in `src-tauri/src/pipeline/actor.rs` with the
150 ms rearm debounce, but no probe run has actually added or removed a device — code landing and
behaviour being exercised are different things, which is the same distinction the watcher correction
above turns on.

**Wispr Flow watcher implemented — FlowWatcher, verified here by source.** `Session::adopt` now has
a production caller at `flow_watcher.rs:195`, where an hour earlier it had none. The module is
macOS-gated, uses a `notify` FSEvents NonRecursive directory watch with a 150 ms debounce
(`DEBOUNCE` at line 49), skips adoption when the session is already valid (line 181), starts from
`setup()` and stops at `RunEvent::Exit`. AUT-032's Verify moved from `e2e` to `unit`, justified
rather than accommodated: the 7 tests do not stub the watcher — they call `FlowWatcher::watch` and
then `fs::write` the file, so real FSEvents delivery is exercised over a temp directory. The tray
half stays TRY-005's, via `lib::watch_session`'s subscription as the single publisher of
`session:changed`.

These three rows stay `todo` regardless: the last reported green suite of 512 predates this module,
so no run has yet exercised its tests. An implementation announcement plus tests that exist is still
not a test run — the third time in this session a row would otherwise have gone green on landed code
rather than executed evidence.

**Pipeline e2e harness — PipelineE2e.** `cargo test --workspace` 541 passing / 0 failed (up from
512), clippy 0 warnings, rustfmt clean. Seventeen rows closed against named tests in
`src-tauri/src/pipeline/tests.rs` (48 to 58 tests, fake-driven): HTK-040, HTK-041, HTK-047, HTK-049
(pipeline half), CTX-009, WSS-003, AUD-024, LIF-005, AUD-032, DIC-008, POL-025, POL-029, POL-030,
POL-032, POL-034, POL-035, POL-036. DIC-008 also pinned the ORDER — a cache warm-up before the seed
would have cached an empty vocabulary. POL-035 reads the polish table back over a second connection
to assert `app == ""`, which is why the test database became file-backed in a tempdir.

*Mutation-tested, which is what makes the above worth trusting:* 18 mutations, 15 killed, 3 survived
and were dealt with rather than explained away. HTK-040's concurrency clause was VACUOUS as first
written — a job issued one line early still logs after an instant fake `start()` — fixed by giving
the fake a configurable device-open latency. HTK-041's gate mutation was a no-op and was
reformulated to genuinely hoist the gate. The third is recorded in the row rather than here: the
prewarm's position inside `start_recording` is NOT observable, because a task spawned from a worker
parks in that worker's non-stealable LIFO slot, so the handshake cannot log until the actor yields
regardless of where the spawn sits — confirmed by moving it to the first statement and watching the
test still pass. PipelineE2e declined to claim it; HTK-040 now excludes that clause and points at
WSS-003.

**HTK-010 is a third false green — no implementation exists.** Reported by SyntheticGuard, confirmed
here: searching `crates/` and `src-tauri/src/` for `on_screen`, `local_display`, `NSScreen`,
`mouse_location` and `screens()` returns only `overlay.rs` comments about overlay POSITIONING.
PORT_PLAN section 4 warns this guard is load-bearing on macOS and must not become a no-op that can
return false. Reopened to `todo`.

**HTK-011 is DEVIATION DV13 — half closed, half deliberately accepted.** Split across two rows rather
than given a fifth status, which is what let each half be ruled on separately. SyntheticGuard found
that OUR OWN injected keystrokes were retriggering OUR OWN hotkey — Natural Mode typing a character
and the tap reading it back as a fresh press, a real bug on the default path that no test in the
suite would have caught. Reproduced live before the fix:

    UNGUARDED: hotkey fired -> HotkeyEvent { binding: Dictate, transition: Pressed }
    ARMED:     no hotkey event

Closed by a 150 ms armed window plus a `0x574C4921` user-data tag — literally the same constant and
mechanism as the Windows side, so the platforms now read alike (HTK-011). The foreign-process half
(HTK-050) is ACCEPTED rather than deferred: `handy-keys` 0.3.3 owns the `CGEvent` and exposes no PID,
and both workarounds were rejected on sound grounds — a second listen-only tap cannot suppress
anything in the first, and an NSEvent monitor fires after OS dispatch where a tap fires before it, so
its answer is always too late. Main accepted it for an independent reason as well: allowing foreign
synthetic input is what keeps Karabiner and other remappers working, and Windows already allows them
deliberately (Q9), so the two platforms are consistent with each other rather than one matching Swift
and the other not.

*A shortcut someone will otherwise retry:* `CGEventSourceStateID::Private` does NOT distinguish our
events. It isolates the modifier state the source reads, not the event's provenance, so it cannot
serve as the marker — which is why the explicit user-data tag exists.

**Watcher rows closed — FlowWatcher.** `cargo test --workspace` 541 passed / 0 failed across all 13
targets, clippy 0, fmt clean, `cargo xwin check -p wispr-lightning` clean. The user's real
`session.json` was untouched: checksum
`87b822745e4125b1f3d6673ef8c6a83adc63cd54ed0dbca734baeacf8741e2d2`, 3399 bytes, mtime unchanged.
AUT-031, AUT-032 and LIF-008 flipped to `done` on this run.

*Unresolved observation, disclosed by FlowWatcher and preserved rather than rounded away.* An earlier
run of theirs reported 122 passed / **1 failed** in the `wispr-lightning` lib target. That run raced
DeviceListeners landing `device_watch` — `wl-platform` went 67 to 74 tests between their two runs and
its doctests were transiently unresolvable — so the failing test name could not be recovered. They
stress-ran their own 7 in isolation, 25/25 then 10/10 with every core saturated, to rule out FSEvents
timing under load; the positive test polls to a 5 s deadline while the whole file runs in 0.64 s, a
margin of roughly two orders of magnitude. The 7 are very unlikely to be the flake, but one failure
in this workspace remains unexplained, and the 541-green figure that many rows in this matrix now
rest on was preceded by it.

**Systematic audit of every `done` row proved by rule rather than by a named test.** Commissioned by
Main after three false greens all traced to one blanket rule — mark `done` anything whose Verify is
`unit`, `fixture` or `contract` on a green crate — which is sound for rows describing code that
exists and silently wrong for rows describing code that does not.

Scope: 428 rows. Method: extract identifiers from each row's Behavior, then look for them across all
80 `.rs` files plus the TypeScript and Svelte sources, and separately across the 46 test files and 86
fixture files. Two candidates surfaced and BOTH were false positives — SET-054's
`written_with_lightning` / `spoken_with_lightning` live in `ui/src/lib/ipc.ts` and
`Dictation.svelte` with the Rust half unit-tested at `settings.rs:603`, and HTK-014's non-modifier
latch is `edge_from` in `windows/matching.rs`, tested at lines 529-541. **No new false green was
found.**

*The limit of that result matters more than the result.* Only 71 of the 428 rows yield an identifier
this method can check at all, because the Behavior column deliberately cites SWIFT symbols — that is
the reference-implementation contract — while the code carries Rust names, so the two do not share
vocabulary. The complementary approach fails the same way: row IDs ARE cited in Rust comments, 103
distinct ones, but only 12 of the 428 audited rows are among them and only 6 appear in a test file.
So roughly 357 rows can be neither confirmed nor refuted mechanically. Taking "reopen anything you
cannot find a test for" literally would reopen 416 rows on absence of TRACEABILITY, which is a
different claim from absence of evidence and would make the document less true, not more.

**The actionable finding is that no row→test link exists.** All three false greens were found by
agents reading source for unrelated reasons — a compile error, a handoff, a synthetic-input fix — and
that is not a repeatable audit. The cheap fix is a convention rather than a tool: cite the row ID in
the test name or a comment above it. The convention already exists informally in 103 places; making
it required would turn reconciliation from judgement into a grep.

**Device listeners — DeviceListeners, implementation verified here by source.** Both system-wide
listeners exist. macOS `crates/wl-platform/src/macos/devices.rs`: two `AudioObjectAddPropertyListener`
registrations on `kAudioObjectSystemObject` for `kAudioHardwarePropertyDevices` and
`kAudioHardwarePropertyDefaultInputDevice`, distinct boxed client-data per registration, added and
removed on one dedicated thread. Windows `crates/wl-platform/src/windows/devices.rs`:
`IMMNotificationClient` via `RegisterEndpointNotificationCallback`, `OnDefaultDeviceChanged` filtered
to eCapture+eConsole, `OnDeviceAdded`/`OnDeviceRemoved`/`OnDeviceStateChanged` for list changes,
joining the process-wide implicit MTA rather than calling `CoInitializeEx`. The fault mapping is
unit-tested at `audio_impl.rs:829-910`, including the non-terminality assertions that keep
`DevicesChanged` from rebuilding a healthy stream.

AUD-019 and AUD-020 nonetheless stay `todo`: their Verify layer is `probe`, and the `device-watch`
probe arm is reported as landing rather than landed, so no run has yet added or removed a device.
Code existing plus a unit test of the mapping is not the same as the OS actually delivering the
callback, which is the specific thing `probe` exists to prove.

AUD-020's behaviour was REWORDED rather than closed: it asserted the Swift log line
`Wispr Lightning: AVAudioEngine configuration changed`, and there is no AVAudioEngine in the port, so
that literal has no analogue and should never have been a parity assertion. The observable behaviour
— reconfiguration invalidates the resolved device and posts audio-devices-changed — is what the row
now claims. AUD-038 was added for the `DefaultChanged` versus `DevicesChanged` distinction, which the
report surfaced and no row had captured.

**Device listeners closed on live OS delivery — DeviceListeners, verbatim.** Two harnesses against a
live `CpalCapture`; nothing calls the listener directly, the only thing invoked is a CoreAudio state
change and the callback arrives on a HAL notification thread.

*List change (`kAudioHardwarePropertyDevices`)* — a private aggregate device created and destroyed,
which is a genuine HAL device add and remove, chosen over a physical unplug because it touches
nothing the user owns:

    baseline faults: []
    create aggregate: status=0 id=115
      after create: [DevicesChanged]
    destroy aggregate: status=0
      after destroy: [DevicesChanged]

*Default-input change (`kAudioHardwarePropertyDefaultInputDevice`)* — the machine default moved to
another real input and put back, with the restore verified by reading the property back rather than
trusting the status code:

    current default input: id=78 uid=Some("BuiltInMicrophoneDevice")
    alternate input: id=83 uid=Some("05EA9D76-64FD-4B7B-9EFD-BA2300000003")
    baseline faults: []
    set default input -> 83: status=0
      after switch: [DefaultChanged]
    restore default input -> 78: status=0
      after restore: [DefaultChanged]
    restored: id=78 uid=Some("BuiltInMicrophoneDevice") (matches original: true)

The empty baselines are what make this proof rather than coincidence: neither fault is emitted
spontaneously, each appears only after the OS state actually changes, and the two properties produce
two DIFFERENT faults — which is precisely AUD-019's claim that the two listeners are distinct.

*Durability caveat, recorded because it is the weakness of this evidence.* These were ad-hoc
harnesses, since deleted, so the proof is this transcript rather than something a later reader can
re-run. That is the same gap the "How a row gets closed" section describes. ProbeExtend's
`device-watch` arm should still land as the durable form; these rows are closed on the observation,
not waiting on it.

## Coverage by verification layer

| Verify | Rows | Share |
|---|---|---|
| `fixture` | 85 | 11.9% |
| `unit` | 301 | 42.1% |
| `contract` | 46 | 6.4% |
| `probe` | 58 | 8.1% |
| `e2e` | 217 | 30.3% |
| `manual` | 4 | 0.6% |
| `n/a` | 4 | 0.6% |
| **Total** | **715** | **100.0%** |

Only **4 of 715 rows (0.6%)** rest on a manual step:

- **AUD-026** — manual: observe the OS microphone indicator with keepMicrophoneActive on and no dictation running
- **AUT-004** — manual: install alongside Wispr Flow on macOS and confirm the deep link still reaches one of the two apps
- **OVL-006** — manual: switch virtual desktops mid-recording on Windows and note whether the overlay follows
- **LIF-023** — manual: install the new bundle over the old one, confirm only one `Wispr Lightning.app` remains in /Applications, then launch and verify the hotkey works with no new permission prompts

The **4 rows marked `n/a`** are behaviors deliberately NOT ported, retired in place so the
decision stays visible. Three are dead-code retirements (DV9, DV10, DV12); the fourth, HTK-050, is an
accepted behavioural deviation (DV13) with a named revisit trigger:

- **HTK-050** — DEVIATION DV13 (accepted half): foreign synthetic input is allowed through — FOREIGN-PROCESS synthetic keystrokes are ACCEPTED, not rejected. The Swift original requires `kCGEventSourceUn...
- **SND-021** — DEVIATION DV10 — RETIRED as dead code: `playPaste()` has exactly one occurrence in the Swift tree — its own definition at Sound...
- **DB-028** — DEVIATION DV9 — RETIRED as dead code: `SELECT COUNT(*), COALESCE(SUM(num_words), 0) FROM transcripts WHERE timestamp >= ?` bou...
- **OVL-025** — DEVIATION DV12 — RETIRED as dead code: the `showRetryableError` 260 px no-Save variant at RecordingOverlay.swift:224, reached o...
