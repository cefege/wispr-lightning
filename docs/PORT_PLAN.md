# Wispr Lightning — Rust/Tauri Cross-Platform Port

Status: **implemented; Deepgram-only cutover complete**

The original Swift application remains the behavioral reference for dictation, storage, hotkeys, text injection, history, dictionary, notes, permissions, sound, and lifecycle behavior. The production application is now Rust + Tauri v2 on macOS and Windows. Transcription has one path: Deepgram Nova streaming.

Historical source-analysis documents under `docs/parity/` explain the Swift implementation that was ported. They are evidence, not live product requirements where they describe retired Wispr Flow, OpenRouter, Claude Voice, OAuth, AI Polish, or fallback-chain behavior.

## Product contract

1. Hold or toggle a configured global hotkey.
2. Capture 16 kHz mono signed 16-bit PCM in 40 ms packets.
3. Stream packets to Deepgram while recording.
4. Send focused-app metadata, accessibility text, optional screen OCR, and dictionary phrases as bounded Nova-3 keyterms.
5. Finalize on release; retry transient failures twice while preserving audio.
6. Apply local formatting, replacements, snippets, signatures, and auto-learn behavior.
7. Insert at the focused cursor without allowing the overlay to steal focus.
8. Save transcript history locally in SQLite.
9. Resume media and return the recording state machine to idle.

## Sole-provider decision

Deepgram is not a selectable backend. The application contains:

- no provider picker, model-vendor registry, capability DTO, or fallback chain;
- no Wispr Flow/Supabase account session, OAuth callback, URL scheme, or filesystem watcher;
- no OpenRouter or Claude Voice implementation or credential path;
- no AI Polish service, hotkey, database table, settings pane, or pipeline branch;
- no provider identity field in settings or transcript results.

`TranscriptionProvider` and `DictationSession` remain only as the pipeline's dependency-injection seam. Production constructs `DeepgramProvider`; deterministic pipeline tests construct a loopback double.

## Architecture

```text
crates/
  wl-core/       settings, SQLite stores, recording FSM, text and migration logic
  wl-platform/   macOS and Windows hotkeys, audio, injection, context, media, permissions
  wl-providers/  Deepgram streaming, local key storage, errors, post-processing
src-tauri/       Tauri shell, lifecycle, IPC, tray/windows, recording orchestration
ui/              Svelte settings, onboarding, history, dictionary, notes, overlay
```

### Platform boundary

`wl-platform` owns every OS-specific operation. macOS uses AppKit/CoreAudio/Accessibility APIs. Windows uses Win32, UI Automation, WASAPI through cpal, SendInput, and native tray/lifecycle APIs. Portable crates contain no platform guesses.

### Deepgram boundary

The Deepgram API key is stored directly in `credentials.json` under the app data directory. The application never opens macOS Keychain or Windows Credential Manager. The UI receives only `configured: bool` and renders a masked saved state.

The request contract is:

- `wss://api.deepgram.com/v1/listen`;
- linear16, 16 kHz, mono;
- Nova-3 model selection;
- explicit language translation, multilingual streaming for auto/multiple languages;
- repeated, validated `keyterm` query parameters;
- `mip_opt_out=true`;
- binary PCM frames, `Finalize`, then `CloseStream`;
- actionable authentication, quota, rate-limit, timeout, and server errors.

## Persistence and migration

Existing macOS users retain `~/Library/Application Support/WisprLightning`. Windows uses `%APPDATA%\WisprLightning`.

On load, settings preserve supported values and remove retired provider/auth/Polish keys. A pre-cutover shared language list is migrated once into `deepgramLanguage`, then removed. SQLite migration 2 removes the retired `polish` table while retaining transcripts, dictionary entries, and notes.

## Verification gates

| Gate | Current evidence |
|---|---|
| Rust behavior | `cargo test --workspace` — 504 tests passed |
| Rust compile | `cargo check --workspace --all-targets` |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Frontend types/build | `pnpm check` and `pnpm build` — 303 files, 0 errors/warnings |
| Windows compile | `cargo xwin check --workspace --all-targets --target x86_64-pc-windows-msvc` |
| macOS bundle | Tauri 2.11 app bundle built from the production frontend and release Rust binary |
| macOS install smoke | Installed binary launched from `/Applications`; startup, migrations, permission checks, hotkey setup, and spool recovery observed in the application log |

Windows runtime behavior still requires a smoke run on Windows hardware. Cross-compilation proves type and build compatibility, not OS permission dialogs, global-input delivery, UI Automation behavior, or installer policy.

## Release procedure

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir ui build
cargo xwin check --workspace --all-targets --target x86_64-pc-windows-msvc
CI=true pnpm dlx @tauri-apps/cli@2.11.1 build --bundles app
```

For Windows release artifacts, run the Tauri bundle step on a Windows runner so NSIS/MSI signing and runtime smoke tests use the real platform.
