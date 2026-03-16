# ⚡ Wispr Lightning

A lightweight macOS dictation app powered by [Wispr Flow](https://wispr.com)'s transcription API. Hold a hotkey, speak, release — your words appear wherever the cursor is.

## Features

- **Push-to-talk dictation** — hold a configurable hotkey to record, release to transcribe and inject text
- **Context-aware formatting** — uses the active app and on-screen text (via OCR) to intelligently format transcriptions
- **Processing indicator** — overlay transitions from "Recording…" → "Processing…" → success/error
- **Music auto-pause** — pauses Apple Music/Spotify during recording, resumes after
- **Transcription history** — browse and search past dictations
- **Menu bar app** — lives in the status bar with a lightning bolt icon

## Requirements

- macOS 13+
- Swift 5.9+
- A [Wispr](https://wispr.com) account

## Install

```bash
./install.sh
```

This builds a release binary, bundles it into `Wispr Lightning.app`, generates the app icon, and copies it to `/Applications`.

### Permissions

After first launch, grant these in **System Settings → Privacy & Security**:

- **Accessibility** — for text injection into other apps
- **Input Monitoring** — for global hotkey capture
- **Microphone** — prompted automatically on first recording

## Build (dev)

```bash
swift build          # debug
swift build -c release  # release
./build-app.sh       # builds .app bundle in current directory
```

## Usage

1. Sign in with your Wispr account (menu bar → Open Wispr Lightning)
2. Hold the hotkey (default: Right Option) and speak
3. Release — text is transcribed and typed at your cursor

## License

Private — not for redistribution.
