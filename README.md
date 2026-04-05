# Wispr Lightning

A lightweight native macOS dictation app powered by [Wispr Flow](https://wispr.com)'s transcription API. Hold a hotkey, speak, release — your words appear wherever the cursor is.

Wispr Lightning is a ground-up rewrite of the Wispr Flow desktop client in native Swift. It uses the same transcription backend but replaces the Electron shell with a lean macOS-native app, eliminating the browser engine overhead entirely.

## Demo

[![Wispr Lightning demo](demo_thumbnail.jpg)](https://www.loom.com/share/e2c4c33d832441fb9ee2383b0305fe54)

## Performance vs. Wispr Flow

Measured on the same machine (macOS 15.3), both apps idle.

| Metric | Wispr Lightning | Wispr Flow | Difference |
|---|---|---|---|
| **RAM (idle)** | 18 MB | ~560 MB | **31× less** |
| **CPU (idle)** | ~0% | ~21% | |
| **Processes** | 1 | 11 | **11× fewer** |
| **App size** | 5.2 MB | 438 MB | **84× smaller** |

Wispr Flow is built on Electron — it ships and runs a full Chromium browser engine to display its UI, spawning 11 OS processes at launch (4 renderers, GPU compositor, network service, audio helper, plugin helper, crash reporter, Swift helper, main shell). Together they consume ~560 MB of RAM while doing nothing.

Wispr Lightning is a single native Swift process. The OS parks it at 0% CPU between interactions.

**Real-world impact:** On a MacBook M1 Air with 8 GB of RAM, Wispr Flow's idle footprint is 7% of total system memory. Under a real dev workload — Chrome with 20 tabs, VS Code, Claude Code, Slack — available RAM shrinks fast and Wispr Flow consistently crashes. Wispr Lightning's 18 MB footprint is negligible under any workload.

## Features

- **Push-to-talk dictation** — hold a configurable hotkey to record, release to transcribe and inject text
- **Context-aware formatting** — uses the active app and on-screen text (via OCR) to intelligently format transcriptions
- **Auto-polish** — optionally rewrites transcriptions with a custom AI prompt before injecting
- **Processing indicator** — overlay transitions from Recording → Processing → done
- **Music auto-pause** — pauses Apple Music / Spotify during recording, resumes after
- **Transcription history** — browse and search past dictations
- **Menu bar app** — lives in the status bar, zero UI clutter

## Requirements

- macOS 13+
- Swift 5.9+
- A [Wispr](https://wispr.com) account

## Install

```bash
./install.sh
```

Builds a release binary, bundles it into `Wispr Lightning.app`, and copies it to `/Applications`.

### Permissions

After first launch, grant these in **System Settings → Privacy & Security**:

- **Accessibility** — for text injection into other apps
- **Input Monitoring** — for global hotkey capture
- **Microphone** — prompted automatically on first recording

## Build

```bash
swift build             # debug
swift build -c release  # release
```

## Usage

1. Sign in with your Wispr account via the menu bar
2. Hold the hotkey (default: Left Control) and speak
3. Release — text is transcribed and typed at your cursor

## Disclaimer

This is an independent project. It is not affiliated with, endorsed by, or connected to [Wispr](https://wispr.com) in any way. "Wispr" and "Wispr Flow" are trademarks of their respective owners. A valid Wispr account and subscription are required to use this application.

## License

Source-available — see [LICENSE](LICENSE). You may view and study the code for personal and educational purposes. Redistribution, commercial use, and derivative works are not permitted.
