# Wispr Lightning — Claude notes

A macOS dictation app. Push-to-talk hotkey records audio, sends it to a backend for transcription, then types or pastes the result into the focused app. Positioned against Wispr Flow.

## Project at a glance

- **Build system:** Swift Package Manager. `swift build` for debug, `swift build -c release` for shipping.
- **Target:** macOS 13+. Single executable target `WisprLightning`.
- **No tests, no CI** — verification is manual via `/smoke`.
- **Entry point:** `Sources/WisprLightning/App/AppDelegate.swift`. The status bar UI lives in `App/StatusBarController.swift`. The pill overlay is `UI/RecordingOverlay.swift`. Settings model + window in `Models/Settings.swift` and `UI/SettingsWindow.swift`. Audio capture, transcription client, and text injection live in `Services/`.

## Build and run

```
swift build -c release      # compile
./build-app.sh              # produce Wispr Lightning.app (note: script may still reference the old "Wispr Lite" name)
./install.sh                # copy to /Applications/
open "/Applications/Wispr Lightning.app"
```

`/smoke` wraps the build → install → launch → log-watch sequence.

## Logs and crash reports

- **Live log:** `~/Library/Logs/WisprLightning.log` — written by `wLog(_:)` in `App/AppDelegate.swift`. Tail it with `tail -F` while exercising the app.
- **Crash reports:** `~/Library/Logs/DiagnosticReports/WisprLightning-*.ips`.

If you're investigating a bug the user is seeing, start here. The log is the cheapest signal source in the project.

## Self-improvement loop

The repo has three project-level slash commands that compose into a propose → pick → ship cycle. The user is the picker; you (Claude) are the proposer and implementer.

- **`/propose`** — spawns three Explore agents in parallel (log-detective, code-archaeologist, product-strategist), each scanning a different signal source. Returned candidates are deduped against existing items and appended to `BACKLOG.md` with stable IDs (`B-001`, `B-002`, …). IDs are never reused.
- **`/improve <id>`** — picks one item from `BACKLOG.md`, summarizes it back, implements, verifies (`swift build`, then `/smoke` for runtime changes), and on user confirmation marks the item `Status: done (commit <sha>)` and commits.
- **`/smoke`** — quits the running app, rebuilds, reinstalls, relaunches, and tails the log while the user exercises the change. Reports anomalies.

`BACKLOG.md` is the durable artifact between sessions. `MEMORY.md` (in `~/.claude/projects/.../memory/`) holds session-spanning lessons.

## Conventions and prior lessons

These are things the codebase looks like it does for a reason. Don't undo them without asking.

- **Natural Mode typed output must mirror the transcript verbatim.** `Services/TextInjector.swift` uses `CGEventSource(stateID: .privateState)` and unconditionally pins `event.flags` to exactly what the layout map specifies. Do not let physical Caps Lock, residual shift from the dictation hotkey, or any other ambient modifier ride along. The user observed Caps Lock flipping case, comma → `<`, apostrophe → `"` with the older `.hidSystemState` + `if !flags.isEmpty` shape; pinning fixed all three.
- **Newlines in Natural Mode are sent as Shift+Return**, not bare Return. This prevents accidental message submission in chat / terminal apps.
- **Esc cancels Natural Mode typing** mid-stream. Implementation uses NSEvent global + local monitors (no CGEventTap) and a thread-safe `cancelLock` + `_cancelTyping` flag in `TextInjector`. The local monitor swallows the keystroke so it doesn't reach the focused app.
- **Pill state must be reset before each `TextInjector.inject` call.** Call `recordingOverlay.showInserting()` first; otherwise prior states (Retrying yellow, error buttons) bleed through. There are four inject call sites in `AppDelegate.swift`.
- **First-letter capitalization is the backend AI formatter, not Natural Mode.** If the user reports "it's capitalizing my first word", that's the transcription pipeline applying formatting; don't go looking in `TextInjector`.
- **`build-app.sh` may still output as "Wispr Lite"** (legacy name). Renaming is its own backlog item, not a side-effect of unrelated work.
- **AX context is reliably empty.** `TextInjector.readFocusedElementText()` reads `kAXValueAttribute` on the focused element. That attribute is unset or non-string in most modern apps (Slack, Cursor, Claude Code, terminals, web chat composers, document editors), so the runtime log shows `AX context: none` essentially always. This is the same root cause as the dropped paste verification. Don't treat it as a small bug — fixing it well needs either an AX hierarchy walk with per-bundle-ID heuristics or a pivot to OCR-only context. `useAccessibilityContext` defaulting to `true` is currently aspirational.

## Guidance for Claude

- When the user reports a bug, check the log before reading code.
- Default to editing existing files. Don't add files, abstractions, or comments the task doesn't need.
- For runtime / UI changes, never claim success without `/smoke` and a user confirmation. `swift build` only proves it compiles.
- The user prefers terse responses and direct fixes. They will tell you if they want more.
