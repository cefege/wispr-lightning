---
description: Propose backlog items for Wispr Lightning by spawning three coworker agents in parallel
---

You are running the **proposer** half of the self-improvement loop. Read `CLAUDE.md` if you haven't already; it describes the loop. Your job here: gather signal from three independent sources and append new items to `BACKLOG.md`.

## Step 1 — read existing backlog

Read `BACKLOG.md` (it may not exist yet — that's fine, you'll create it). Note the highest existing `B-NNN` ID so new items continue from `max + 1`. Note open items by title so you can dedupe.

## Step 2 — spawn three agents in parallel

Send a single message with three `Agent` tool calls. Use `subagent_type: "Explore"` for all three. Pass each one a self-contained prompt — they don't see this conversation.

### Agent A — log-detective

> Read `~/Library/Logs/WisprLightning.log` (last ~500 lines via `tail -500`) and the most recent crash report under `~/Library/Logs/DiagnosticReports/WisprLightning-*.ips` if any exist. Identify: recurring error/warn lines, retry loops, slow paths (> 1s), unexpected state transitions, crashes. For each candidate, return: a one-line title, a 2-line evidence quote (with log line numbers or timestamps), severity guess (low/medium/high), and a rough scope (files likely involved, LOC estimate). Return 2–3 candidates. Be specific — "improve error handling" is not a candidate; "showRetrying state persists after successful retry on slow networks" is."

### Agent B — code-archaeologist

> Read the last 20 commits via `git log --oneline -20` and `git log -p --since='14 days ago' -- Sources/`. Then scan `Sources/WisprLightning/` for: duplicated logic, dead code, inconsistent patterns, files that needed a hotfix recently, places where a TODO/FIXME would belong but doesn't exist. Return 2–3 refactor candidates. For each: one-line title, evidence (file paths + line numbers + brief quote of the smell), risk (low/medium/high — does this touch hot paths?), scope estimate. Skip cosmetic preferences; flag things where the current shape is actively making bugs more likely."

### Agent C — product-strategist

> Read `README.md`, `Sources/WisprLightning/UI/SettingsWindow.swift`, and `Sources/WisprLightning/Models/Settings.swift`. Wispr Lightning is a macOS dictation app — push-to-talk hotkey, transcribes via a backend, types or pastes the result into the focused app. It positions itself against Wispr Flow. Identify 2–3 feature candidates: gaps vs. Flow, low-hanging UX wins, capabilities that the existing settings/UI hint at but don't implement. For each: one-line title, value (why this matters to a daily user), rough scope (which files would change). Don't propose features that already exist — read the settings UI carefully first."

## Step 3 — assemble and append

When all three return:

1. Dedupe against existing open items in `BACKLOG.md` — if a returned candidate matches an open item, skip it (mention to the user that you skipped it).
2. Assign sequential IDs starting from `max + 1` (never reuse). Bug, refactor, and feature can interleave — order is just by ID.
3. Append each new item to `BACKLOG.md` using this exact format:

```markdown
## B-007 — One-line title

- **Type:** bug | refactor | feature
- **Severity / Value:** low | medium | high
- **Evidence:** <file:line or log timestamp + brief quote>
- **Scope:** <files involved, LOC estimate>
- **Status:** open
```

If `BACKLOG.md` doesn't exist yet, create it with this header at the top:

```markdown
# Wispr Lightning — Self-improvement backlog

Items proposed by `/propose`, picked off by `/improve <id>`. See `CLAUDE.md` for the loop overview.

---
```

Then the items.

## Step 4 — report back to the user

Print a compact table: ID, type, severity, one-line title. Mention any candidates that were skipped as duplicates. Do **not** commit — let the user review first. If they say to commit, use a message like `Propose B-007..B-013`.

## Guidance

- If an agent returns weak candidates ("add more tests", "improve docs"), drop them rather than padding the backlog.
- If two agents return the same item from different angles, keep one and note the cross-reference in the evidence line.
- A backlog of 6 strong items beats one of 12 mediocre items.
