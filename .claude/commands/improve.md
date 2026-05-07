---
description: Pick a backlog item, implement it, verify, and ship
argument-hint: <backlog-id, e.g. B-007>
---

You are running the **implement** half of the self-improvement loop. The user invoked `/improve $ARGUMENTS` to carry one backlog item from "open" to "shipped".

## Step 1 — locate the item

Read `BACKLOG.md`. Find the entry whose ID matches `$ARGUMENTS` (case-insensitive, leading `B-` optional). If nothing matches, list the open items and ask the user which one they meant. If the entry is already `Status: done`, ask whether to redo it or pick a different one.

## Step 2 — summarize and align

Read whatever files the entry's evidence/scope lines reference. Then summarize back to the user, in 3–5 lines:

- What the item is asking for, in your words.
- The approach you'd take.
- Any non-obvious tradeoff or scope question.

Wait for the user to confirm or redirect before editing files. If their response materially changes scope, restate it once before proceeding — don't ping-pong.

## Step 3 — implement

Default: edit files directly in this session. Use `Agent` with `isolation: "worktree"` if the change is exploratory or you want to compare two approaches without polluting the working tree.

Stay within the scope listed in the backlog item. If you discover the real fix is bigger than the entry suggests, stop and tell the user — they may want to re-scope the entry or split it.

## Step 4 — verify

- Always: `swift build` (allow-listed in `.claude/settings.local.json`).
- For UI / runtime / hotkey changes: ask the user to run `/smoke` and report what they see. **Type checking is not feature checking.** Do not claim success on UI work without the user confirming the behavior.
- If the change touches the typing path (`TextInjector`), explicitly confirm Natural Mode still mirrors the transcript exactly — there's a memory note about this.

## Step 5 — ship

Once the user confirms it works:

1. In `BACKLOG.md`, change the item's `Status: open` → `Status: done (commit <pending>)`. (Update the sha after committing.)
2. Stage the implementation files + `BACKLOG.md`.
3. Commit with a message that names the item: `B-007: <one-line title>`. Body: 2–3 lines on what changed and why.
4. Replace `<pending>` in `BACKLOG.md` with the actual sha and amend, OR commit the sha update separately — your call. Don't push unless the user asks.

## Guidance

- Don't pre-emptively run `/simplify` or `/ultrareview` — the user invokes those. You can suggest one if the diff is sprawling.
- If verification reveals the fix made things worse, revert with `git restore` and report — don't try to patch on top of a broken first attempt.
- Keep the commit focused on the item. Unrelated cleanup goes in its own commit or its own backlog entry.
