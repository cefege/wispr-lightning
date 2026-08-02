# Wispr Lightning

Push-to-talk dictation for **macOS and Windows**. Hold a key, speak, release — the transcribed text lands at your cursor in whatever app you were already using.

Originally a native Swift macOS app; now a Rust + Tauri v2 desktop app with one transcription path: Deepgram live streaming.

## What it does

```
Hotkey pressed
  ├─ Capture the frontmost app and optional text/screen context
  ├─ Pause music
  ├─ Open a Deepgram WebSocket
  └─ Stream 16 kHz mono PCM while recording

Hotkey released
  ├─ Finalize the Deepgram transcript
  ├─ Apply local replacements, snippets, capitalization, and punctuation
  ├─ Insert at the cursor
  ├─ Save to local SQLite history
  └─ Resume music
```

**Push-to-talk, tap-to-lock.** Hold to dictate. Tap twice quickly and it locks hands-free until you press again. Releasing after a real hold waits half a second before stopping, so the tail of your sentence is never clipped.

**Contextual recognition.** With Nova 3, dictionary phrases and distinctive terms from the focused app, field text, and optional screen OCR are sent as Deepgram keyterm hints.

**Your own dictionary.** Vocabulary phrases bias recognition; replacements and snippets rewrite the output. Words the recognizer gets wrong but the formatter fixes are learned automatically.

**Local application data.** History, notes, dictionary, settings, and the Deepgram key stay in the app data folder. The app never opens macOS Keychain or Windows Credential Manager.

## Deepgram

Wispr Lightning streams headerless 16 kHz mono PCM to Deepgram's `/v1/listen` WebSocket. Nova 3 is the default model; Nova 2 remains selectable. Fixed-language, multilingual, and streaming auto-detect modes are available.

Deepgram's `smart_format` and `dictation` options provide basic formatting and spoken punctuation. The app then applies dictionary replacements and snippets locally, followed by sentence capitalization and terminal punctuation. Nova 3 can also receive up to 500 tokens of contextual keyterm hints.

Setup requires a Deepgram API key. The key is saved locally and write-only in the UI: after saving, settings display a masked state rather than revealing it.

## Install

Download the latest release: `.dmg` for macOS 13+, `.msi` or `.exe` for Windows 10/11.

### First-launch permissions

Setup requests each required permission in sequence and cannot be skipped. The app advances only
after the operating system reports that the current request is granted.

**macOS** — approve these in System Settings → Privacy & Security:
- **Microphone** — required to record dictation
- **Accessibility** — required to insert text into other apps
- **Input Monitoring** — required for the global hotkey
- **Screen Recording** — required while screen context is enabled

**Windows** — enable *Settings → Privacy & security → Microphone → Let desktop apps access your microphone*. There is no per-app prompt for desktop apps; setup links you straight to that page if it detects a denial.

Settings → Privacy shows every permission's live status with a button to request or open the relevant pane. If the hotkey ever stops working, that page tells you why.

## Build from source

Requires Rust 1.85+, Node 20+, and pnpm.

```bash
pnpm --dir ui install
cargo tauri dev      # run
cargo tauri build    # package
```

### Cross-compiling to Windows from macOS

```bash
brew install llvm
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cargo xwin check -p wl-platform --target x86_64-pc-windows-msvc
```

## Layout

```
crates/wl-core/       settings · SQLite · audio framing · recording state machine · text
crates/wl-providers/  Deepgram streaming · credential storage · local post-processing
crates/wl-platform/   hotkeys · capture · injection · OCR · media — macOS + Windows
src-tauri/            tray · windows · overlay · IPC · dictation pipeline
ui/                   Svelte 5 + Vite — settings, history, notes, dictionary, overlay
```

`wl-core` and `wl-providers` touch no OS APIs, so the bulk of the behaviour is testable on any host.

## Testing

```bash
cargo test --workspace
pnpm --dir ui check
cargo run -p wl-platform --example probe
```

The verification layers are:

1. **Unit and contract tests** — recording state transitions, settings migration, SQLite stores, text transforms, Deepgram URL construction, keyterms, WebSocket streaming, retry classification, and local post-processing.
2. **Mock Deepgram server tests** — streaming audio, finalization, keep-alive, malformed frames, authentication failures, timeouts, and reconnect behavior.
3. **Platform probe** — microphones, hotkeys, injection, OCR, media control, and permissions on the real OS.
4. **End-to-end smoke** — the installed binary, including the invariant that the recording overlay never takes focus from the app receiving dictation.

## Requirements

- macOS 13+ (Apple Silicon or Intel) or Windows 10/11
- A [Deepgram](https://deepgram.com) API key

## Disclaimer

Independent project. Not affiliated with, endorsed by, or connected to Deepgram or Wispr.

## License

Source-available — see [LICENSE](LICENSE). You may view and study the code for personal and educational purposes. Redistribution, commercial use, and derivative works are not permitted.
