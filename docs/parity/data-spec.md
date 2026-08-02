# Wispr Lightning — Data / Auth / Polish / Transcription-Tail Behavioral Spec

All line references are to `Sources/WisprLightning/`. Everything below was read verbatim from source; nothing is inferred unless marked `[INFERENCE]`.

---

## 1. `Services/DatabaseManager.swift` — storage root and migration

### On-disk paths (exact)

| Purpose | Path |
|---|---|
| App support dir | `~/Library/Application Support/WisprLightning` (`FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/WisprLightning")`) |
| Legacy DB | `~/Library/Application Support/WisprLightning/history.db` |
| Active DB | `~/Library/Application Support/WisprLightning/lightning.db` |
| Settings | `~/Library/Application Support/WisprLightning/settings.json` |
| Own session | `~/Library/Application Support/WisprLightning/session.json` |
| Foreign session (read/watch) | `~/Library/Application Support/Wispr Flow/session.json` (note the space in `Wispr Flow`) |

### init() behavior — trigger -> action -> result

1. **App start** -> `createDirectory(at: dir, withIntermediateDirectories: true)` (errors swallowed via `try?`) -> dir exists.
2. **`history.db` exists AND `lightning.db` does NOT exist** -> `moveItem(history.db -> lightning.db)` -> logs `"Wispr Lightning: Migrated history.db → lightning.db"` (note: real U+2192 arrow). If `lightning.db` already exists, `history.db` is left orphaned on disk, never deleted.
3. `sqlite3_open(lightning.db)` -> on `SQLITE_OK`: run `PRAGMA journal_mode=WAL;` and log `"Wispr Lightning: Database opened at %@"`. On failure: `db = nil`, log `"Wispr Lightning: Failed to open database at %@"`, and **every store silently no-ops** (all `sqlite3_prepare_v2` guards return early with empty results).

### Helpers

- `exec(_ sql:)` -> `sqlite3_exec(db, sql, nil, nil, nil)`; **return code ignored**.
- `transaction(_ block:)` -> `exec("BEGIN TRANSACTION;")`, run block, `exec("COMMIT;")`. **No rollback path exists.**
- `columnText(stmt, index) -> String?` — `nil` when the column is SQL NULL.
- `bindOptionalText(stmt, index, value)` — binds text or `sqlite3_bind_null`.
- `close()` -> `sqlite3_close(db)`; called from `applicationWillTerminate` after `historyStore.close()` (which is a no-op).

### Migration / versioning

**There is no `user_version`, no `ALTER TABLE`, no `CREATE INDEX` anywhere in the codebase** (verified by grep for `CREATE INDEX|ALTER TABLE|PRAGMA|user_version` — only hit is the WAL pragma). Schema evolution is entirely `CREATE TABLE IF NOT EXISTS` executed by each store's `init`. Consequence for the port: an older DB that lacks a newer column will NOT be upgraded; parity requires reproducing this (or doing better, deliberately).

---

## 2. EXACT SQLite schema — all four `CREATE TABLE` statements verbatim

Each is issued via `dbManager.exec(...)` from the corresponding store's `init` (so table creation order is whatever order `AppDelegate` constructs the stores: HistoryStore, DictionaryStore, PolishStore, NotesStore).

### 2.1 `transcripts` (HistoryStore.createTable)

```sql
CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY,
    asr_text TEXT,
    formatted_text TEXT,
    timestamp REAL,
    app_name TEXT,
    app_bundle_id TEXT,
    duration REAL,
    num_words INTEGER,
    language TEXT
);
```

### 2.2 `dictionary` (DictionaryStore.createTable)

```sql
CREATE TABLE IF NOT EXISTS dictionary (
    id TEXT PRIMARY KEY,
    phrase TEXT NOT NULL,
    replacement TEXT,
    team_dictionary_id TEXT DEFAULT '00000000-0000-0000-0000-000000000000',
    last_used REAL,
    frequency_used INTEGER DEFAULT 0,
    manual_entry INTEGER DEFAULT 0,
    created_at REAL NOT NULL,
    modified_at REAL NOT NULL,
    is_deleted INTEGER DEFAULT 0,
    source TEXT,
    is_snippet INTEGER DEFAULT 0,
    UNIQUE(phrase, team_dictionary_id)
);
```

Notes: `last_used` is declared but **never written or read** anywhere. `frequency_used` is written only as the literal `0` on insert and **never incremented** — so `ORDER BY frequency_used DESC` is effectively an arbitrary/insertion-order ordering today. `team_dictionary_id` is never bound explicitly; every row gets the default `'00000000-0000-0000-0000-000000000000'`, which makes the UNIQUE constraint behave as UNIQUE(phrase).

### 2.3 `polish` (PolishStore.createTable)

```sql
CREATE TABLE IF NOT EXISTS polish (
    id TEXT PRIMARY KEY,
    initial_text TEXT,
    polished_text TEXT,
    initial_word_count INTEGER,
    polished_word_count INTEGER,
    app TEXT,
    processing_time REAL,
    status TEXT,
    polish_undone INTEGER DEFAULT 0,
    instruction TEXT,
    created_at REAL NOT NULL,
    updated_at REAL NOT NULL
);
```

### 2.4 `notes` (NotesStore.createTable)

```sql
CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    content_preview TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at REAL NOT NULL,
    modified_at REAL NOT NULL,
    is_deleted INTEGER DEFAULT 0,
    finalized INTEGER DEFAULT 0
);
```

`finalized` is declared but never written or read.

### Type conventions across all tables

- Every `id` is a `TEXT` uppercase `UUID().uuidString` (Foundation format, e.g. `E621E1F8-C36C-495A-93FC-0C247A3E6E5F`) — except `transcripts.id` and `polish.id`, which are the UUIDs generated by `TranscriptionClient` (`transcriptUUID`) and `PolishService` (`polishUUID`) respectively.
- All timestamps are `REAL` = **Unix epoch seconds as a Double** (`Date().timeIntervalSince1970`), NOT milliseconds, NOT Apple reference date.
- All booleans are `INTEGER` 0/1.

---

## 3. Every query, verbatim, with ordering and limits

### 3.1 HistoryStore (`Services/HistoryStore.swift`)

**`addEntry(result:appInfo:language:)`**, `language` defaults to `"en"`:
```sql
INSERT OR REPLACE INTO transcripts
(id, asr_text, formatted_text, timestamp, app_name, app_bundle_id, duration, num_words, language)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);
```
Bindings: 1=`result.id`; 2=`result.asrText` (NULL-able); 3=`result.formattedText` (NULL-able); 4=`Date().timeIntervalSince1970` **at insert time, not recording time**; 5=`appInfo["name"] ?? ""`; 6=`appInfo["bundle_id"] ?? ""`; 7=`result.duration`; 8=`Int32(result.numWords)`; 9=`language`.

**`getEntries(limit: Int = 100, offset: Int = 0)`**:
```sql
SELECT * FROM transcripts ORDER BY timestamp DESC LIMIT ? OFFSET ?;
```
Relies on `SELECT *` returning declaration column order 0..8 (id, asr_text, formatted_text, timestamp, app_name, app_bundle_id, duration, num_words, language).

**`search(query:)`** — case-insensitivity comes from SQLite's default `LIKE` on ASCII only:
```sql
SELECT * FROM transcripts WHERE formatted_text LIKE ? OR asr_text LIKE ? ORDER BY timestamp DESC LIMIT 100;
```
Pattern = `"%\(query)%"` bound to both params. **No escaping of `%` or `_` in the user query** — literal wildcards typed by the user are honored.

**`deleteEntry(id:)`** (hard delete): `DELETE FROM transcripts WHERE id = ?;`

**`clearAll()`**: `DELETE FROM transcripts;`

**`todayStats() -> (dictations: Int, words: Int)`**:
```sql
SELECT COUNT(*), COALESCE(SUM(num_words), 0) FROM transcripts WHERE timestamp >= ?;
```
Bound param = `Calendar.current.startOfDay(for: Date()).timeIntervalSince1970` — **local-timezone midnight, current system calendar**. Returns `(0,0)` if prepare or step fails.

Row mapping (`entryFromRow`): missing text -> `""` for `id`/`appName`/`appBundleId`, `"en"` for `language`; `asrText`/`formattedText` stay `Optional`.

### 3.2 NotesStore (`Services/NotesStore.swift`)

**`addNote(title: String = "Untitled", content: String = "") -> String`** (returns the new UUID even if the insert fails):
```sql
INSERT INTO notes (id, title, content_preview, content, created_at, modified_at)
VALUES (?, ?, ?, ?, ?, ?);
```
`content_preview = String(content.prefix(200))` — 200 **Swift Characters (extended grapheme clusters)**, not bytes, not UTF-16 units. `created_at == modified_at == now`.

**`updateNote(id:title:content:)`**:
```sql
UPDATE notes SET title = ?, content_preview = ?, content = ?, modified_at = ? WHERE id = ?;
```
Preview recomputed as `prefix(200)` on every update.

**`softDelete(id:)`**: `UPDATE notes SET is_deleted = 1, modified_at = ? WHERE id = ?;`

**`getNotes(limit: Int = 100)`**:
```sql
SELECT id, title, content_preview, content, created_at, modified_at FROM notes WHERE is_deleted = 0 ORDER BY modified_at DESC LIMIT ?;
```

**`search(query:)`**:
```sql
SELECT id, title, content_preview, content, created_at, modified_at FROM notes WHERE is_deleted = 0 AND (title LIKE ? OR content LIKE ?) ORDER BY modified_at DESC LIMIT 100;
```
Pattern `"%\(query)%"`, unescaped, bound twice. Search matches full `content`, not the preview.

### 3.3 PolishStore (`Services/PolishStore.swift`)

**Write-only store. There are no SELECT statements at all** — nothing in the app ever reads the `polish` table.

**`saveResult(_ result: PolishResult, app: String = "")`**:
```sql
INSERT OR REPLACE INTO polish
(id, initial_text, polished_text, initial_word_count, polished_word_count, app, processing_time, status, instruction, created_at, updated_at)
VALUES (?, ?, ?, ?, ?, ?, ?, 'completed', ?, ?, ?);
```
`status` is the **hard-coded SQL literal `'completed'`** (not a bound param) — failures are never persisted. Bindings: 1=`result.id`, 2=`initialText`, 3=`polishedText`, 4=`initialWordCount`, 5=`polishedWordCount`, 6=`app`, 7=`processingTime` (seconds, Double), 8=`instruction`, 9=`created_at`=now, 10=`updated_at`=now. `polish_undone` is never written (stays default 0).

Call sites: manual polish -> `polishStore.saveResult(polishResult, app: appInfo["name"] ?? "")`; auto-polish -> `polishStore.saveResult(polishResult)` i.e. `app = ""`.

### 3.4 DictionaryStore (`Services/DictionaryStore.swift`)

**`addEntry(phrase:replacement:isSnippet:source:manualEntry:)`** — `source` defaults `"manual"`, `manualEntry` defaults `true`:
```sql
INSERT OR IGNORE INTO dictionary
(id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at)
VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?);
```
`INSERT OR IGNORE` + `UNIQUE(phrase, team_dictionary_id)` means **re-adding an existing phrase is a silent no-op** (it does NOT update the replacement, does NOT bump modified_at). A new `UUID().uuidString` is generated for the id on every attempt. Calls `invalidateCache()` afterwards regardless of whether the row was inserted.

**`updateEntry(id:phrase:replacement:)`**: `UPDATE dictionary SET phrase = ?, replacement = ?, modified_at = ? WHERE id = ?;` -> invalidate cache.

**`softDelete(id:)`**: `UPDATE dictionary SET is_deleted = 1, modified_at = ? WHERE id = ?;` -> invalidate cache. **Soft-deleted rows keep occupying the UNIQUE(phrase) slot**, so re-adding a deleted phrase silently fails (INSERT OR IGNORE) and the phrase can never be resurrected through the UI.

**`getVocabularyPhrases(limit: Int = 50) -> [String]`** (memoized in `cachedVocabulary`):
```sql
SELECT phrase FROM dictionary WHERE is_snippet = 0 AND is_deleted = 0 ORDER BY frequency_used DESC LIMIT ?;
```

**`getReplacements() -> [String: String]`** (memoized in `cachedReplacements`, **no LIMIT**):
```sql
SELECT phrase, replacement FROM dictionary WHERE is_snippet = 0 AND replacement IS NOT NULL AND is_deleted = 0;
```

**`getSnippets() -> [String: String]`** (memoized in `cachedSnippets`, **no LIMIT, no ORDER BY**):
```sql
SELECT phrase, replacement FROM dictionary WHERE is_snippet = 1 AND is_deleted = 0;
```
Rows with NULL `replacement` are skipped by the `if let` in the loop.

**`fetchEntries(snippet:)`** — backing `getAllVocabulary()` (snippet=false) and `getAllSnippets()` (snippet=true):
```sql
SELECT id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at FROM dictionary WHERE is_snippet = ? AND is_deleted = 0 ORDER BY modified_at DESC;
```
**No LIMIT.** Explicit column list -> row indices 0..8 map to `DictionaryEntry`.

**`searchEntries(query:snippet:)`**:
```sql
SELECT id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at FROM dictionary WHERE is_snippet = ? AND is_deleted = 0 AND phrase LIKE ? ORDER BY modified_at DESC;
```
Pattern `"%\(query)%"`; searches `phrase` only, never `replacement`.

### Cache semantics (parity-critical)

Three independent `Optional` caches (`cachedVocabulary`, `cachedReplacements`, `cachedSnippets`). Populated lazily on first call. `invalidateCache()` nils **all three** and is called by `addEntry`, `updateEntry`, `softDelete` only. Caches are **not thread-safe** (no lock) and are read from `TranscriptionClient` on whatever queue builds the auth message while the UI mutates them on the main queue. Warm-up: `AppDelegate` calls `seedDefaults(...)` then `getVocabularyPhrases()`, `getReplacements()`, `getSnippets()` on startup so the first dictation doesn't pay the query cost.

---

## 4. DictionaryStore data model and output shapes

### Model (`Models/DictionaryEntry.swift`)
```swift
struct DictionaryEntry: Identifiable, Hashable {
    let id: String
    let phrase: String
    let replacement: String?
    let isSnippet: Bool
    let manualEntry: Bool
    let source: String?
    let frequencyUsed: Int
    let createdAt: Date
    let modifiedAt: Date
}
```
`==` and `hash` are **id-only** (two entries with identical text but different ids are distinct; an edited entry stays "equal" to its old value).

### The three logical kinds, disambiguated purely by two columns

| Kind | `is_snippet` | `replacement` | Sent to server as |
|---|---|---|---|
| Vocabulary phrase (spelling hint / proper noun) | 0 | NULL | `dictionary_context: [String]` |
| Replacement (auto-correct pair) | 0 | non-NULL | `dictionary_replacements: {phrase: replacement}` |
| Snippet (expansion) | 1 | non-NULL | `dictionary_snippets: {phrase: [replacement]}` |

A vocabulary row **with** a replacement appears in BOTH `dictionary_context` (phrase only) and `dictionary_replacements` — the two queries overlap by design.

### Output shaping at the call site (`TranscriptionClient` auth message)

```swift
"dictionary_context":      dictionaryStore?.getVocabularyPhrases() ?? []          // [String], max 50
"dictionary_replacements": dictionaryStore?.getReplacements() ?? [:]             // {String: String}
"dictionary_snippets":     (dictionaryStore?.getSnippets() ?? [:]).mapValues { [$0] }  // {String: [String]}
```
Note the snippet map values are **wrapped in a single-element array** — the wire format is `{"phrase": ["expansion"]}`, not `{"phrase": "expansion"}`.

### CSV import (`importCSV(url:) -> (imported: Int, errors: [String])`)

1. Read file as UTF-8; on failure return `(0, ["Failed to read file"])`.
2. Split on `CharacterSet.newlines` (handles \n, \r\n, \r, U+2028/2029).
3. Per line: trim whitespace; skip empty lines (no error, no counter).
4. **Header skip**: only for `index == 0`, and only if the lowercased line contains `"phrase"` OR `"abbreviation"`.
5. Split on `,`. `phrase = parts[0]` trimmed of whitespace then of `"` characters. `replacement` = `parts[1...]` re-joined with `,` (so replacements may contain commas), trimmed of whitespace then `"`; `nil` when there is only one field.
6. Empty phrase -> error `"Line \(index + 1): empty phrase"`, continue.
7. `isSnippet = (replacement != nil)` — **any two-column CSV row becomes a snippet, never a replacement**.
8. `addEntry(phrase:replacement:isSnippet:source: "csv_import")` (manualEntry defaults to true); `imported += 1` even if `INSERT OR IGNORE` dropped the row as a duplicate.

The `"Line \(index + 1): invalid format"` branch is unreachable (`components(separatedBy:)` always yields ≥1 element).

### Auto-learn words

- `addAutoLearnedWord(phrase:)` -> `addEntry(phrase:, replacement: nil, isSnippet: false, source: "user_edits", manualEntry: false)`.
- `addAutoLearnedWords(phrases:)` -> returns immediately on empty; otherwise wraps the loop in `dbManager.transaction { ... }` (BEGIN/COMMIT, no rollback). Each iteration calls `invalidateCache()` (redundant but harmless).

**The learning algorithm lives in `AppDelegate.autoLearnWords(asrText:formattedText:)`** and runs only when `settings.autoLearnWords == true` AND both `asrText` and `formattedText` are non-nil (i.e. AI formatting produced a different text):

1. `asrWords = Set(asrText.lowercased().split(separator: " ").map(String.init))` — whitespace split on the literal space only.
2. For each word in `formattedText.split(separator: " ")`:
   - skip if `asrWords.contains(word.lowercased())` (not a correction);
   - `cleaned = word.trimmingCharacters(in: .punctuationCharacters)`;
   - require `cleaned.count > 2` **and** `cleaned.first?.isUppercase == true`;
   - append `cleaned`.
3. If non-empty -> `dictionaryStore.addAutoLearnedWords(phrases: wordsToLearn)` and log `"Auto-learned \(n) words"`. Duplicates within one batch and against existing rows are absorbed by `INSERT OR IGNORE`.

### `seedDefaults(userName:)`

Called once at startup. If `userName` is non-nil and non-empty -> `addEntry(phrase: name, replacement: nil, isSnippet: false, source: "default", manualEntry: false)`. Then unconditionally `addEntry(phrase: "Wispr Lightning", replacement: nil, isSnippet: false, source: "default", manualEntry: false)`.

### `source` value vocabulary (exact strings)
`"manual"` (default), `"csv_import"`, `"user_edits"` (auto-learned), `"default"` (seeded).

---

## 5. PolishService (`Services/PolishService.swift`)

### ⚠️ Correction to the brief: there is NO system prompt and NO model name in this app.

The client never sends prompt text or a model identifier. It sends the **instruction labels themselves** as a `{String: Bool}` map and the server (`api.wisprflow.ai`) owns the prompt and model choice. `custom_prompt` is explicitly `null`. The only "prompt text" that exists client-side is the seven instruction label strings in `AppSettings.polishInstructions` (§8).

### Endpoint
`POST https://api.wisprflow.ai/llm/polish_text` (built as `"\(Constants.apiURL)/llm/polish_text"`, `Constants.apiURL == "https://api.wisprflow.ai"`).

### Preconditions -> failures
1. `text.isEmpty` -> `.failure(.emptyResult)` ("No transcription returned").
2. `!session.isValid` -> `session.refresh { }`; on refresh failure (or self deallocated) -> `.failure(.authFailed)`; on success -> proceed.
3. Malformed URL or `JSONSerialization` failure -> `.failure(.connectionFailed)`.

### Request headers (exact)
```
Content-Type: application/json
Authorization: <raw access token>          // NO "Bearer " prefix — comment says "Wispr Flow sends the raw token without \"Bearer\" prefix"; empty string when token is nil
Cache-Control: no-cache, no-store, must-revalidate
```

### Request body (exact JSON keys; `JSONSerialization`, key order therefore unspecified)
```json
{
  "selected_text": "<text>",
  "instructions": { "<instruction label>": true, ... },
  "provider_config": null,
  "writing_samples": null,
  "custom_prompt": null
}
```
Built as `instructions.reduce(into: [String: Bool]()) { $0[$1] = true }` from the **active** instruction list — i.e. only enabled instructions are present and each maps to `true`; disabled ones are omitted entirely (never sent as `false`). The `[String]` passed in is always `settings.activePolishInstructions` = `polishInstructions.filter { $0.value }.map { $0.key }` (**unordered** — Swift dictionary iteration order).

Side effect: `wLogVerbose("Polish request body: <json>")` when `verboseLogging` is on.

### Timeout
**None is set.** Plain `URLSession.shared.dataTask` -> default `timeoutIntervalForRequest` = **60 s**, `timeoutIntervalForResource` = 7 days. Contrast with the WebSocket path, which implements its own timeout.

### Response parsing (every field read)
- transport error non-nil -> log `"Wispr Lightning: Polish request failed: %@"` -> `.connectionFailed`.
- `data == nil` or JSON is not a top-level object -> log `"Wispr Lightning: Polish response parse failed"` -> `.connectionFailed`. **HTTP status code is never inspected.**
- `wLogVerbose("Polish response: <raw body>")`.
- `json["polished_text"] as? String`, non-empty -> success. Otherwise read `json["status"] as? String ?? "unknown"`, log `"Wispr Lightning: Polish failed with status: %@"`, return `.serverError(status)` (user message `"Server error: <status>"`).

**Only two response keys are ever read: `polished_text` and `status`.**

### Success result (`PolishResult`)
```swift
id: polishUUID                       // UUID().uuidString generated before the request
initialText: text
polishedText: polishedText
initialWordCount:  text.split(separator: " ").count          // space-split, empty subsequences omitted
polishedWordCount: polishedText.split(separator: " ").count
processingTime: Date().timeIntervalSince(startTime)          // seconds, wall clock across the whole request
instruction: instructions.joined(separator: ". ")            // NOTE: ". " separator, no trailing period
```
The completion handler is invoked on the **URLSession delegate queue**, not the main queue; callers hop to main themselves.

### Two call sites (`AppDelegate`)

**Manual polish hotkey** (`onPolishHotkeyPress`, default hotkey Right Control = keycode 62):
1. Guard `settings.polishEnabled`; guard `!activePolishInstructions.isEmpty` (else log `"Polish: no instructions enabled"` and abort).
2. `AppInfoDetector.getFrontmostAppInfo()`; `soundManager.playStart()`; `recordingOverlay.show()` + `.showProcessing()`.
3. Off-main: save clipboard; synthesize **Cmd+C** via `CGEvent(keyboardEventSource:virtualKey: 8, keyDown:)` with `.maskCommand`, posted to `.cghidEventTap` (virtual key 8 = `C`).
4. `Thread.sleep(forTimeInterval: 0.15)` — **150 ms** fixed wait for the target app to fill the pasteboard.
5. Read `NSPasteboard.general.string(forType: .string)` on the main queue synchronously. Empty/nil -> restore clipboard, `showError(message: "Select text to polish")`, abort.
6. On success: `textInjector.inject(polishedText)`, then after **0.3 s** restore the original clipboard, `playStop()`, `overlay.hide()`; then `polishStore.saveResult(polishResult, app: appInfo["name"] ?? "")`.
7. On failure: restore clipboard, `recordingOverlay.showError(message: error.userMessage)`.

**Auto-polish after dictation** (`autoPolishText`, when `settings.autoPolish && settings.polishEnabled && !activeInstructions.isEmpty`): the raw transcript injection is skipped and the overlay stays in Processing; on success inject `polishedText` then hide overlay, and `polishStore.saveResult(polishResult)` (app = `""`); on failure log and **inject the original text** as fallback, then hide overlay.

---

## 6. AuthService (`Services/AuthService.swift`) + Session

### 6.1 Sign-in flow — trigger -> action -> result

**Trigger**: Settings window button `"Sign In with Google"` -> `AuthService.signInWithBrowser()`.

```swift
let redirectURI = "wispr-flow://auth/google/success"
let encodedRedirect = redirectURI.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? redirectURI
let authURL = "\(Constants.supabaseURL)/auth/v1/authorize?provider=google&redirect_to=\(encodedRedirect)"
NSWorkspace.shared.open(url)
```
Concrete URL opened in the user's default browser:
```
https://dodjkfqhwrzqjwkfnthl.supabase.co/auth/v1/authorize?provider=google&redirect_to=wispr-flow://auth/google/success
```
(`.urlQueryAllowed` leaves `:` and `/` unescaped, so the redirect appears literally.)

**Callback scheme**: `wispr-flow` — **deliberately shared with the commercial Wispr Flow app**. `Resources/Info.plist` registers `CFBundleURLSchemes = ["wispr-flow", "wisprlightning"]` under bundle identifier `com.wisprlightning.app`. Whichever app macOS routes the URL to wins; the loser recovers via the file watcher (§6.4).

### 6.2 Callback handling (`AppDelegate.handleURLEvent` -> `AuthService.handleCallback`)

Registration: `NSAppleEventManager.shared().setEventHandler(self, andSelector: #selector(handleURLEvent(_:withReplyEvent:)), forEventClass: AEEventClass(kInternetEventClass), andEventID: AEEventID(kAEGetURL))`.

Filter: `guard urlString.contains("auth/")` — all other `wispr-flow://` deep links are ignored. Logs `"Wispr Lightning: Received URL callback: %@"`.

`handleCallback(url:session:completion:)`:
1. Parse `URLComponents(url:resolvingAgainstBaseURL: false).queryItems` into `params` (last duplicate wins).
2. **Fragment fallback** when `params["access_token"] == nil`: split `url.fragment` on `&`, then each pair on `=` with `maxSplits: 1`, applying `removingPercentEncoding` to the value.
3. Require `params["access_token"]` and `params["refresh_token"]` — otherwise `completion(false)`.
4. Assign `session.accessToken`, `session.refreshToken`, `session.expiresAt = Double(params["expires_at"] ?? "0") ?? 0`.
5. Decode the JWT payload **without signature verification** (`decodeJWTPayload`): split token on `.`; require ≥2 segments; base64url -> base64 (`-`->`+`, `_`->`/`), pad with `=` until `count % 4 == 0`; `Data(base64Encoded:)`; `JSONSerialization`. Read `sub` -> `userId`, `email` -> `userEmail`, and inside `user_metadata`: `avatar_url` ?? `picture` -> `avatarURL`; `full_name` ?? `name` ?? `""` split on space with `maxSplits: 1` -> `userFirstName` / `userLastName` (falling back to query params `first_name` / `last_name`).
6. Post-fallback: if `userFirstName`/`userLastName` still nil, take `params["first_name"]` / `params["last_name"]`.
7. `session.save()` then `completion(true)`.

On `true`, AppDelegate logs `"Wispr Lightning: Sign in successful"` and posts `Notification.Name("WisprSessionChanged")` on the main queue; on `false` logs `"Wispr Lightning: Sign in failed"`.

Callback query params consumed: `access_token`, `refresh_token`, `expires_at`, `first_name`, `last_name`.

### 6.3 How the token reaches `Session` and is persisted (`Models/Session.swift`)

Fields: `accessToken`, `refreshToken`, `userId`, `userEmail`, `userFirstName`, `userLastName`, `avatarURL`, `expiresAt: TimeInterval`, and `let sessionId = UUID().uuidString` (regenerated per process launch; sent as `metadata.session_id` on every transcription).

`isValid`: `accessToken != nil` AND (`expiresAt == 0` OR `now <= expiresAt - 60`) — **60-second skew**.

`load()`: try `liteSessionURL` first; if that fails try `wisprFlowSessionURL` and, on success, `save()` into Lightning's own file and log `"Wispr Lightning: Migrated session from Wispr Flow (%@)"`.

`parseSession`: find the **first key containing the substring `"auth-token"`**; its value is either a JSON *string* to be re-parsed or a nested object. Then read `access_token`, `refresh_token`, `expires_at`, `user.id`, `user.email`, `user.user_metadata.{avatar_url|picture, full_name|name}`. Fails if `access_token` is nil. If `avatarURL == nil || userEmail == nil`, `enrichFromJWT` fills `email`, `exp` (used as the authoritative `expiresAt` when in the future), and metadata.

`refresh(completion:)`:
```
POST https://dodjkfqhwrzqjwkfnthl.supabase.co/auth/v1/token?grant_type=refresh_token
Content-Type: application/json
apikey: <Constants.supabaseAnonKey>
Authorization: Bearer <Constants.supabaseAnonKey>
body: {"refresh_token": "<refreshToken>"}
```
Requires `access_token` and `refresh_token` in the response. `expiresAt` = `expires_at` if it is in the future, else `now + expires_in`, else 0; then `enrichFromJWT` may override from `exp`. Then `save()`, verbose-log the first 300 chars of the response, log `"Wispr Lightning: Token refreshed successfully"`, `completion(true)`. Any failure -> log `"Wispr Lightning: Token refresh failed: %@"` -> `completion(false)`. **No timeout override (default 60 s), no retry.**

`save()` — writes `~/Library/Application Support/WisprLightning/session.json`, pretty-printed, with the inner object serialized as a **JSON string**, mirroring Supabase's browser storage format:
```json
{
  "sb-dodjkfqhwrzqjwkfnthl-auth-token": "{\"access_token\":\"...\",\"refresh_token\":\"...\",\"expires_at\":1234567890,\"user\":{\"id\":\"...\",\"email\":\"...\",\"user_metadata\":{\"full_name\":\"First Last\",\"avatar_url\":\"...\"}}}"
}
```
`full_name` is always `"\(userFirstName ?? "") \(userLastName ?? "")"` — a bare space when both are nil. `avatar_url` is omitted when nil. **Plain file, no keychain, no encryption, no file-permission hardening.**

`clear()`: nils all fields, `expiresAt = 0`, deletes `session.json`, posts `WisprSessionChanged`. Note it does NOT clear `avatarURL`.

### 6.4 Wispr Flow session watcher (`AppDelegate.startWisprFlowSessionWatcher`)

`open(<~/Library/Application Support/Wispr Flow>, O_EVTONLY)` + `DispatchSource.makeFileSystemObjectSource(eventMask: [.write, .rename], queue: .main)`. On event: skip if `session.isValid`; else `session.load()` (which migrates + saves), log `"Wispr Lightning: Picked up session from Wispr Flow (%@)"`, post `WisprSessionChanged`, `statusBarController.updateMenu()`. Cancel handler closes the fd. The directory is created first if missing. Torn down in `applicationWillTerminate`.

---

## 7. TranscriptionClient tail (`Services/TranscriptionClient.swift`)

### 7.1 Constants that govern the protocol (`Services/Constants.swift`)
```swift
supabaseURL   = "https://dodjkfqhwrzqjwkfnthl.supabase.co"
supabaseAnonKey = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImRvZGprZnFod3J6cWp3a2ZudGhsIiwicm9sZSI6ImFub24iLCJpYXQiOjE3MTk4ODQzMDcsImV4cCI6MjAzNTQ2MDMwN30.h6EeQ_6kqFeznH25icVUX0Szn9__kc8HoSXAsxxBWG8"
wsURL         = "wss://api.wisprflow.ai/llm/ws"
apiURL        = "https://api.wisprflow.ai"
sampleRate    = 16000
channels      = 1
chunkDurationMs = 40
chunkSamples  = 640            // sampleRate * chunkDurationMs / 1000
clientVersion = "1.4.549"
maxRecordingSeconds = 600
warningSeconds = 540
finalWarningSeconds = 570
```
Socket setup: `URLRequest(url: wsURL)` with header `Encoding: json`; `task.maximumMessageSize = 10 * 1024 * 1024` (10 MB).
Client-side chunking: `private static let chunkSize = 500` packets ("~20 seconds of audio, ~800KB encoded").

### 7.2 Message 1 — `auth` (context for the tail; keys verbatim)
```json
{
  "type": "auth",
  "access_token": "<token or \"\">",
  "app": "<appInfo.type lowercased, default \"other\">",
  "context": {
    "app": { "name": "", "bundle_id": "", "type": "", "url": "" },
    "ax_context": [],
    "ocr_context": [],
    "dictionary_context": [],
    "dictionary_replacements": {},
    "dictionary_snippets": {},
    "user_first_name": "",
    "user_last_name": "",
    "textbox_contents": {},
    "content_text": "",
    "variable_names": [],
    "file_names": []
  },
  "personalization_style_settings": {},
  "language": ["en"],
  "metadata": {
    "session_id": "<Session.sessionId>",
    "environment": "PRODUCTION",
    "client_platform": "darwin",
    "client_version": "1.4.549",
    "transcript_entity_uuid": "<transcriptUUID>"
  },
  "pipeline": ["transcribe", "format"],
  "job_selectors": [],
  "cleanup_level": "light",
  "command_mode": true,
  "debug_mode": false,
  "use_staging_baseten": false,
  "prefix_is_written": false,
  "hyperlink_on": false
}
```
Variants: `pipeline` = `["transcribe", "format"]` when `settings.aiFormatting` else `["transcribe"]`; `personalization_style_settings` = `settings.personalizationStyles` when `styleDetectionEnabled` else `{}`; `job_selectors` = `["creator"]` when `settings.creatorMode` else `[]`; `prefix_is_written` = `!axContext.isEmpty`. The server must reply with `status == "auth"` or the client cancels with `.internalServerError` and fails `.authFailed`.

### 7.3 Message 2..N — `append` (exact shape, per chunk)

`sendNextChunk(offset:)`, recursive; `end = min(offset + 500, totalPackets)`, `isFinal = end >= totalPackets`:
```json
{
  "type": "append",
  "audio_packets": {
    "packets": ["<ascii85 string>", ...],
    "volumes": [0.1234, ...],
    "packet_duration": 0.04,
    "audio_encoding": "wav",
    "byte_encoding": "ascii85"
  },
  "position": <offset>,
  "final": <bool>
}
```
- `packet_duration` = `Double(Constants.chunkDurationMs) / 1000.0` = **0.04**.
- `audio_encoding` is the literal string `"wav"` even though the payload is **raw headerless 16-bit little-endian mono PCM at 16 kHz**, 640 samples = 1280 bytes per packet. No WAV header is prepended for the socket path (the WAV header builder in AppDelegate is only for on-disk debug dumps).
- `position` is the packet index of the chunk's first packet (0, 500, 1000, ...), NOT a byte offset.
- `volumes[i]` = RMS normalized: `sumSquares` over `Int16` samples, `rms = sqrt(sumSquares / sampleCount)`, value = `(rms / 32768.0 * 10000).rounded() / 10000` -> **4-decimal-place** number in `[0.0, 1.0]`. `.rounded()` is half-away-from-zero. (Corrected 2026-08-01: an earlier revision of this document claimed a ceiling of 0.3052, which was an arithmetic slip — full-scale input gives `32767/32768*10000 = 9999.7`, which rounds to `10000`, i.e. `1.0`. Verified against the formula in CPython.)
- Chunks are sent strictly sequentially: each `send` completion triggers the next. Any send error -> log `"Wispr Lightning: WS chunk send failed: %@"` -> `.connectionFailed`.
- Verbose log per chunk: `"WS sending chunk \(offset)..<\(end) of \(totalPackets) (\(appendString.count) bytes, final=\(isFinal))"`.

### 7.4 Message N+1 — `commit`
```json
{ "type": "commit", "total_packets": <totalPackets> }
```
Sent only after the final `append`'s send completion. Send error -> log `"Wispr Lightning: WS commit send failed: %@"` -> `.connectionFailed`. On success logs `"Wispr Lightning: Audio sent — %d packets in %d chunks, waiting for transcription..."` with `chunkCount = (totalPackets + 500 - 1) / 500`, then starts the receive loop.

### 7.5 Response timeout
```swift
responseTimeout(for packetCount) = max(15.0, Double(packetCount) * 40 / 1000.0 * 0.5)
```
i.e. **max(15 s, half the recorded duration)**. Implemented as a `DispatchWorkItem` scheduled on `DispatchQueue.global(qos: .userInitiated)` at `.now() + timeout`. On fire: log `"Wispr Lightning: WebSocket response timed out after %.0fs"`, `wsTask.cancel(with: .abnormalClosure, reason: nil)`, complete `.timeout`. The work item is cancelled when a result arrives. Double-completion is prevented by a `completed` flag under `NSLock` (`safeComplete`) — present in both `performTranscription` and `receiveResultWithTimeout`.

Separately, `AppDelegate` runs a UI-level processing timeout of `max(30.0, 30.0 + recordingDuration * 0.5)` seconds.

### 7.6 Response parsing — EVERY field read

Only `.string` messages are handled; a `.data` message falls through **without re-arming the receive loop** (the connection then hangs until the timeout fires). Non-JSON strings behave the same way.

Fields read from the top-level object:

| Key | Type | Meaning / effect |
|---|---|---|
| `status` | String? | dispatch: `"auth"` (auth phase only), `"text"`, `"error"`, `"info"`; anything else -> ignored, loop continues |
| `body` | Object? -> `[:]` | container for the transcript, only when `status == "text"` |
| `body.llm_text` | String? | AI-formatted text -> `TranscriptResult.formattedText` |
| `body.asr_text` | String? | raw ASR text -> `TranscriptResult.asrText` |
| `final` | Bool? -> `false` | top-level (NOT inside body); `true` ends the stream |
| `error` | String? -> `"unknown"` | when `status == "error"` -> `.serverError(detail)` |
| `message` | String? -> `""` | when `status == "info"` -> logged only |

Behavior:
- `status == "text"`: `resultText = llmText ?? asrText ?? ""`; log `"Wispr Lightning: Got %@ transcript: %d chars"` with `"final"`/`"partial"`. If `final == false`, fall through and re-arm receive (partials are logged, not surfaced).
- `final == true`: `duration = Double(packetCount) * 40 / 1000.0`; `numWords = resultText.split(separator: " ").count`; build `TranscriptResult(id: transcriptUUID, asrText:, formattedText:, duration:, numWords:)`; `wsTask.cancel(with: .normalClosure, reason: nil)`; if `resultText.isEmpty` -> `.failure(.emptyResult)` else `.success(result)`. Note `asrText`/`formattedText` are stored as received, so `formattedText` is nil when only ASR ran.
- `status == "error"`: log `"Wispr Lightning: Server error: %@"`, `cancel(with: .internalServerError, reason: nil)`, `.failure(.serverError(detail))`.
- `status == "info"`: log `"Wispr Lightning: Server info: %@"`, continue receiving.
- `receive` failure: log `"Wispr Lightning: WS receive failed: %@"` -> `.connectionFailed`.
- Verbose: `"WS received: \(text.prefix(500))"`.

### 7.7 Error taxonomy (`Models/TranscriptEntry.swift`)
```swift
enum TranscriptionError: Error { case authFailed, connectionFailed, serverError(String), timeout, emptyResult }
```
| Case | `isRetryable` | `userMessage` (verbatim) |
|---|---|---|
| `authFailed` | false | `"Authentication failed — please sign in again"` |
| `connectionFailed` | true | `"Connection failed — check your network"` |
| `serverError(detail)` | true | `"Server error: \(detail)"` |
| `timeout` | true | `"Request timed out — server did not respond"` |
| `emptyResult` | false | `"No transcription returned"` |
(Em-dashes are literal U+2014.)

Trigger inventory: empty packet list -> `.emptyResult`; token refresh failure -> `.authFailed`; auth reply `status != "auth"` or non-string -> `.authFailed`; socket creation / JSON serialization / any send error / receive failure -> `.connectionFailed`; missing prepared audio -> `.connectionFailed`; `status == "error"` -> `.serverError`; deadline -> `.timeout`; empty final text -> `.emptyResult`.

### 7.8 Encoding cache
`cachedEncoding: (packetCount: Int, prepared: PreparedAudio)?` — reused when `cached.packetCount == packets.count`, so **retries do not re-encode**. Keyed only on packet count (a different recording with the same packet count would collide; in practice cleared by `clearEncodingCache()` between dictations). Encoding runs on `DispatchQueue(label: "com.wisprlightning.encode", qos: .userInitiated)` in parallel with the auth round trip, joined by a `DispatchGroup.notify`.

### 7.9 The custom Ascii85 encoder — exact algorithm

Comment in source: `// MARK: - Ascii85 Encoding (matching Python's base64.a85encode)`. It is the classic **btoa/Adobe base-85 charset without Adobe framing**:

1. Output buffer reserved as `(byteCount / 4 + 1) * 5`.
2. Iterate `i` over the input in **4-byte groups**. `remaining = min(4, byteCount - i)`.
3. Build `value: UInt32` big-endian: `for j in 0..<4 { value <<= 8; if j < remaining { value |= UInt32(bytes[i+j]) } }` — i.e. a short final group is **zero-padded on the right** to 4 bytes.
4. **Zero-group shortcut**: if `remaining == 4 && value == 0` -> emit the single byte `0x7A` (`'z'`). This is applied ONLY to full 4-byte groups; a short all-zero tail group takes the normal path (producing `!`-runs), matching CPython's `a85encode` padding rule.
5. Otherwise compute five digits by repeated `% 85` / `/= 85`, **least-significant first into slot 4 and working back to slot 0**, each digit offset by **+33** (`'!'` = 0x21). Charset is therefore the contiguous ASCII range `!` (0x21) .. `u` (0x75), standard Ascii85 — **no z85, no RFC1924, no custom alphabet**.
6. Emit `outputCount = remaining < 4 ? remaining + 1 : 5` bytes from slot 0 forward — i.e. a partial tail group of *n* bytes emits *n+1* characters, truncating the low-order digits (standard Ascii85 tail rule).
7. **No `<~` / `~>` Adobe delimiters, no line wrapping, no whitespace, no `y` space-fold.** Result is `String(bytes: output, encoding: .ascii) ?? ""`.

Input per packet: exactly 1280 bytes (640 Int16 samples) -> 320 full groups -> at most 1600 chars, fewer where a group is all zeros (digital silence yields `z`, so silent audio compresses heavily). All output bytes are in 0x21..0x75 plus 0x7A, so the ASCII conversion never fails.

Rust equivalent: `ascii85` "btoa"-style encoding with `z` compression enabled and no delimiters — verify the `z`-only-for-full-groups rule and the tail truncation rule, both of which naive crates get wrong.

---

## 8. Settings (`Models/Settings.swift`)

### 8.1 Verbatim `polishInstructions` default dictionary
```swift
var polishInstructions: [String: Bool] = [
    "Make more concise": true,
    "Reword for clarity": true,
    "Maintain your tone": true,
    "Reorder for readability": true,
    "Add structure for readability": true,
    "Clarify main point": false,
    "Refine phrasing for impact": false
]
```
Five default-on, two default-off. `activePolishInstructions: [String]` = `polishInstructions.filter { $0.value }.map { $0.key }` — a computed property, **not encoded** (Codable synthesis ignores computed properties), and **unordered**.

### 8.2 File path and persistence format
```swift
static let settingsURL: URL = {
    let dir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/WisprLightning")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.appendingPathComponent("settings.json")
}()
```
-> `~/Library/Application Support/WisprLightning/settings.json`. **UserDefaults is not used for settings at all**; a single JSON file is the entire store.

`load()`: if the file is missing, unreadable, or fails `JSONDecoder().decode(AppSettings.self)` -> construct a fresh `AppSettings()`, immediately `save()` it, and return it. **Any decode error silently resets ALL settings to defaults** — a single unknown-but-non-optional shape, or a removed key, wipes the user's configuration. (`JSONDecoder` tolerates *extra* keys but not *missing* non-optional ones.)

`save()`: `JSONEncoder().encode(self)` -> re-parse with `JSONSerialization` -> re-serialize with `.prettyPrinted` -> `try? pretty.write(to: settingsURL)` (falls back to the compact data if pretty printing fails). Writes are non-atomic (`Data.write(to:)` without `.atomic`) and all errors are swallowed. Then unconditionally `NotificationCenter.default.post(name: .settingsChanged, object: self)`.

Encoded key names are the Swift property names verbatim (no `CodingKeys`, no key-encoding strategy). Pretty-printed output has **no `.sortedKeys`**, so key order is `JSONSerialization`'s hash order, not declaration order.

### 8.3 Full default settings inventory (property : default)
```
hotkeyKeyCode: UInt16 = 59                  // Left Ctrl (legacy single-key)
hotkeyLabel: String = "Left Control"        // legacy single-key
hotkeyKeyCodes: [UInt16] = [59]
hotkeyLabels: [String] = ["Left Control"]
micDeviceUID: String? = nil                 // nil = system default
micDeviceName: String? = nil
keepMicrophoneActive: Bool = false
languages: [String] = ["en"]
launchAtLogin: Bool = false
showInDock: Bool = false
enableSounds: Bool = true
muteMusic: Bool = false
aiFormatting: Bool = true
autoCleanupLevel: String = "light"
commandModeEnabled: Bool = true
useScreenContext: Bool = false
useAccessibilityContext: Bool = true
shareUsageData: Bool = false
styleDetectionEnabled: Bool = true
personalizationStyles: [String: String] = ["work": "default", "email": "default", "personal": "default", "other": "default"]
hyperlinkOn: Bool = false
autoLearnWords: Bool = true
polishEnabled: Bool = false
polishInstructions: [String: Bool] = <see 8.1>
autoPolish: Bool = false
polishHotkeyKeyCodes: [UInt16] = [62]       // Right Control
polishHotkeyLabels: [String] = ["Right Control"]
emailAutoSignature: Bool = false
emailSignatureOption: String = "written_with_lightning"
creatorMode: Bool = false
selectedSoundPack: String? = nil
verboseLogging: Bool = false
hotkeyPaused: Bool = false
naturalModeEnabled: Bool = false
naturalModeSpeed: String = "normal"         // "slow" | "normal" | "expert"
```
Keycodes are **macOS Carbon virtual keycodes**: 59 = Left Control, 62 = Right Control. Polish uses virtual key 8 = `C` for the synthetic Cmd+C.

### 8.4 Notification names (verbatim raw values)
```swift
static let settingsChanged     = Notification.Name("WisprLightningSettingsChanged")
static let sessionChanged      = Notification.Name("WisprSessionChanged")
static let previewSoundPack    = Notification.Name("WisprPreviewSoundPack")
static let audioDevicesChanged = Notification.Name("WisprAudioDevicesChanged")
```
Note the inconsistent prefixes (`WisprLightning...` vs `Wispr...`) — irrelevant to a Rust port except that these are the app's internal event-bus topics: settings-changed, session-changed, sound-pack-preview, audio-device-list-changed.

---

## 9. macOS-only APIs in the files reviewed — and what they DO

| Behavior needed | macOS API used | Where |
|---|---|---|
| Locate the per-user app-data directory | `FileManager.homeDirectoryForCurrentUser` + hard-coded `Library/Application Support/...` | DatabaseManager, Settings, Session |
| Open the system browser to a URL | `NSWorkspace.shared.open(url)` | AuthService.signInWithBrowser |
| Receive the OAuth deep-link callback | `NSAppleEventManager.setEventHandler(forEventClass: kInternetEventClass, andEventID: kAEGetURL)` (Carbon AppleEvents) + `CFBundleURLSchemes` in Info.plist | AppDelegate.handleURLEvent |
| Detect that another app wrote a session file | `DispatchSource.makeFileSystemObjectSource` on an `open(dir, O_EVTONLY)` fd (kqueue) | AppDelegate.startWisprFlowSessionWatcher |
| Copy the user's current selection out of an arbitrary foreign app | `CGEventSource(stateID: .hidSystemState)` + synthetic Cmd+C `CGEvent` posted to `.cghidEventTap` (CoreGraphics; requires Accessibility permission) | AppDelegate.onPolishHotkeyPress |
| Read the copied text | `NSPasteboard.general.string(forType: .string)` (AppKit) | same |
| Identify the frontmost app for context/history | `AppInfoDetector.getFrontmostAppInfo()` (NSWorkspace/AX under the hood) | AppDelegate |
| Structured system logging | `NSLog` | everywhere |
| Local-midnight boundary for "today's stats" | `Calendar.current.startOfDay(for:)` | HistoryStore.todayStats |
| Grapheme-cluster prefix for note previews | Swift `String.prefix(200)` (Unicode-correct, not UTF-16) | NotesStore |
| Global hotkey capture (Left/Right Control) | HotkeyListener (CGEventTap / Carbon keycodes) | out of scope files, but drives Polish |
| HTTP + WebSocket | `URLSession` / `URLSessionWebSocketTask` (portable concepts, Apple implementation) | PolishService, Session, TranscriptionClient |

SQLite itself is portable (`import SQLite3` links the OS-provided library; on Windows the port must bundle SQLite — e.g. `rusqlite` with the `bundled` feature).

---

## 10. Parity risks on Windows

1. **URL scheme ownership is shared with a third-party app.** `wispr-flow://` is registered by both Wispr Lightning and the commercial Wispr Flow, and the Supabase `redirect_to` hard-codes it. On Windows, protocol handlers are a per-user/per-machine registry key (`HKCU\Software\Classes\wispr-flow\shell\open\command`) with **last-writer-wins** and no "foreground app wins" arbitration, so the Wispr Flow fallback path behaves differently. **Recommendation:** use a loopback HTTP redirect (`http://127.0.0.1:<port>/auth/callback`) if the Supabase project allows adding a redirect URL; otherwise register `wisprlightning://` and accept losing the Wispr Flow handoff.
2. **The Wispr Flow session-file watcher has no Windows analogue in path or semantics.** `~/Library/Application Support/Wispr Flow/session.json` -> `%APPDATA%\Wispr Flow\session.json` `[INFERENCE]`; the kqueue directory watch maps to `ReadDirectoryChangesW` (or the `notify` crate), but the parent directory may not exist and Wispr Flow's Windows build may use a different storage format entirely. Treat this feature as macOS-only unless the Windows Wispr Flow format is confirmed.
3. **Polish's "grab the selection" relies on synthesizing Cmd+C and sleeping 150 ms.** Windows needs `SendInput` with Ctrl+C plus clipboard read/restore (`OpenClipboard`/`GetClipboardData`/`SetClipboardData`), and the 150 ms fixed delay is fragile on both platforms — consider clipboard-sequence-number polling (`GetClipboardSequenceNumber`) with a deadline instead of a blind sleep. Also mirror the clipboard save/restore ordering: restore happens 300 ms *after* TextInjector's own restore.
4. **Virtual keycodes 59 / 62 / 8 are Carbon-specific.** Windows equivalents: `VK_LCONTROL` (0xA2), `VK_RCONTROL` (0xA3), `'C'` (0x43). Persisted `hotkeyKeyCodes` in an existing `settings.json` are macOS codes; the port needs either a platform-tagged keycode field or a migration.
5. **Storage root divergence.** `~/Library/Application Support/WisprLightning` -> `%APPDATA%\WisprLightning` (or Tauri's `app_data_dir`). The `history.db` -> `lightning.db` rename migration only matters on macOS but should be kept there for existing installs.
6. **Plaintext token file.** `session.json` holds a live refresh token with default file permissions. On Windows, `%APPDATA%` is user-scoped but not encrypted; consider DPAPI (`CryptProtectData`) / macOS Keychain. Any change alters the on-disk format, so keep read-compat with the existing Supabase `sb-<ref>-auth-token` stringified-JSON shape.
7. **`Authorization` without `Bearer` on `/llm/polish_text`.** Easy to "fix" accidentally in Rust (many HTTP helpers add the prefix). It must remain the raw token.
8. **No HTTP status-code checking anywhere.** A 401/500 with a JSON body lacking `polished_text` surfaces as `.serverError("unknown")`. Reproducing exactly means ignoring the status code; improving it changes observable error text.
9. **Ascii85 tail and `z` rules.** Off-by-one in the partial-group output count, or applying `z` to a padded final group, silently corrupts audio for the server. Unit-test against CPython `base64.a85encode` output for lengths 1..8 and an all-zero buffer.
10. **`SELECT *` positional column mapping in HistoryStore.** Any future column added to `transcripts` before position 8 breaks row decoding. The Rust port should name columns explicitly (behavior-identical, strictly safer).
11. **Unsynchronized dictionary caches** are read from a background encoding/auth path while the UI mutates them. In Rust this is a compile error, so use `Mutex`/`RwLock` or `arc-swap`; the observable contract is only "caches invalidate on add/update/soft-delete".
12. **`LIKE` patterns are unescaped.** `%`/`_` typed by the user act as wildcards. Preserve or fix deliberately — parity says preserve.
13. **Soft-deleted dictionary rows permanently block re-adding the same phrase** (UNIQUE + INSERT OR IGNORE). This is a latent bug; decide explicitly whether to port it.
14. **`Calendar.current.startOfDay`** must map to local-timezone midnight (e.g. `chrono::Local::now().date_naive().and_hms(0,0,0)`), not UTC, or "today's stats" drifts by the UTC offset.
15. **String word counting** uses `split(separator: " ")` with empty subsequences omitted — Rust's `split_whitespace()` differs (it also splits on tabs/newlines). Use `split(' ').filter(|s| !s.is_empty())` for byte-identical counts.
16. **`prefix(200)` is 200 grapheme clusters**, not chars or bytes. Rust needs `unicode-segmentation::graphemes(true).take(200)` to match note previews exactly.
17. **No request timeout on the two HTTP calls** (polish, token refresh) — they inherit URLSession's 60 s. `reqwest` has **no** default timeout, so an explicit `.timeout(Duration::from_secs(60))` is required to match.
18. **WebSocket `Encoding: json` request header and 10 MB max message size** must be set explicitly (`tokio-tungstenite` defaults to 64 MiB max message / 16 MiB max frame — set both, plus the custom header on the handshake request).
19. **`client_platform: "darwin"` is hard-coded** in the auth metadata. Sending `"win32"` may change server behavior; test against the real backend before switching, and keep `client_version: "1.4.549"` unless the server requires otherwise.