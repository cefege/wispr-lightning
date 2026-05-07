---
description: Rebuild Wispr Lightning, relaunch it, and watch logs for anomalies
---

You are running a smoke test. The user just made a change and wants to know if the running app behaves. They will do the actual interacting (recording, dictating, observing the pill); you watch the logs.

## Step 1 — quit the running app

```
osascript -e 'quit app "Wispr Lightning"' || true
pkill -x WisprLightning || true
```

Both are allow-listed. The `|| true` is so a "not running" exit doesn't abort the script.

## Step 2 — build and install

```
swift build -c release && ./build-app.sh && ./install.sh
```

`install.sh` copies the built app to `/Applications/Wispr Lightning.app`. If the build fails, stop here, print the error, and let the user fix it before relaunching anything.

## Step 3 — capture a baseline log timestamp

Before launching, record the current end-of-log so you can diff against it later:

```
wc -l ~/Library/Logs/WisprLightning.log
```

Save the line count.

## Step 4 — launch

```
open "/Applications/Wispr Lightning.app"
```

Tell the user the app is up and ask them to exercise the change (be specific — if the item involved Natural Mode, ask them to dictate something with newlines and punctuation; if it involved the pill, ask them to trigger the relevant state).

## Step 5 — watch the log

Run a tailed read of just the new lines after launch:

```
tail -n +<baseline+1> ~/Library/Logs/WisprLightning.log
```

After the user reports back, scan those lines for: `ERROR`, `WARN`, `failed`, `Retrying`, unexpected state transitions, anything that looks anomalous given the change. Report a 2–3 line verdict:

- "Clean — no warnings, no retries, expected state sequence."
- Or: "Saw `<line>` at `<timestamp>` — looks like `<inference>`. Want me to investigate?"

If the user reports a visible bug that the log doesn't reflect, that's important — the log is incomplete and the change may need extra `wLog` calls.

## Guardrails

- **Don't** trigger any GUI dialogs from your end. The browser-tool warning about modal dialogs applies here too — the running app is not yours to click.
- **Don't** kill the running app a second time without asking, especially if the user is mid-dictation.
- If the build fails, the install fails, or the app refuses to launch, stop and report — don't keep retrying.
