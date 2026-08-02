# Golden fixtures — the parity oracle

**Every file in this directory except this README is generated. Do not hand-edit any of it.**

```sh
swift run fixturegen          # regenerates the whole tree in place
```

`MANIFEST.json` records the SHA-256 of every other file, so a manual edit shows up as a
digest mismatch rather than a silent lie.

---

## Why these exist

The Wispr Flow backend (`wss://api.wisprflow.ai/llm/ws`, `https://api.wisprflow.ai/llm/polish_text`)
is a private, unversioned, undocumented third-party API. There is no schema to consult and
no way to ask what a field means. The only evidence of the protocol that exists is the
shipping Swift client in `Sources/WisprLightning/`, and that client is scheduled for
deletion once the Rust port is green.

These fixtures are that evidence, frozen. If the Rust client produces a frame that matches
them, it speaks the protocol. If it doesn't, it doesn't — and no amount of reading the
Rust code will tell you which. This is verification layer 5.1 in `docs/PORT_PLAN.md`.

## How they are produced

`tools/fixturegen/` is a second executable target in the same package. It reaches the
reference implementation two different ways, and the distinction matters when you are
deciding how much to trust a given fixture:

**Linked (executed).** `tools/fixturegen/Reference/` holds symlinks to twelve files under
`Sources/WisprLightning/`. SwiftPM follows them and compiles the *real* code into the tool,
which then runs it. `AppSettings` and its `Codable` conformance, `Session`,
`DatabaseManager`, and all four stores are genuinely executed — the settings key names, the
four `CREATE TABLE` statements and the dictionary queries in these fixtures are not a
description of the reference, they *are* the reference. `PolishService` is also executed:
its request is captured off `URLSession.shared` with a registered `URLProtocol`, so the URL,
method, headers and body bytes in `polish/` were observed, not written down.

**Transcribed.** The four routines that shape the WebSocket protocol — the `auth` frame, the
`append` frame, the `commit` frame and the ascii85 encoder — are `private` on
`TranscriptionClient` and welded to a live `URLSessionWebSocketTask` whose URL is a `static
let` on an `enum`. There is no seam to point them at a local server, and `URLProtocol` does
not intercept WebSocket tasks, so observing the genuine frames would mean talking to the
production backend with a real account token. They are copied character-for-character into
`tools/fixturegen/ProtocolFrames.swift`. `provenance.json` records the SHA-256 of the source
files, so editing the reference turns into a visible diff instead of silent drift.

As an independent check on the transcription that matters most, all ten ascii85 vectors were
verified byte-for-byte against CPython's `base64.a85encode`, which the Swift source names as
the behavior it replicates.

## How to compare against them

- **`.txt`, `.bin`, `.pcm`, `.wav`, `.sql`, `.db` — compare byte-for-byte.** No caveats.
- **`.json` — parse, then compare values.** JSON here is written with **sorted keys**, which
  is *not* what goes on the wire: the reference builds its frames with `JSONSerialization`
  over a bridged Swift `Dictionary`, whose iteration order comes from a per-process hash
  seed. The wire order is genuinely unspecified, so sorted order is the only stable canonical
  form. Slashes are left unescaped and non-ASCII is emitted as raw UTF-8, both matching
  `serde_json`. Every JSON file ends with a single `\n`.
  - The `auth`, `commit` and `polish` fixtures contain no floating-point numbers, so a Rust
    client serialising from a `BTreeMap` will match them byte-for-byte too.
  - The `append` and `pcm` fixtures **do** contain floats (`volumes`, `packet_duration`), and
    number formatting differs between serialisers — `JSONSerialization` writes `1`, serde
    writes `1.0`. Compare those numerically.

## Normalized fields

Two values change on every run and are replaced with literal placeholder strings:

| Placeholder | Real value | Where |
|---|---|---|
| `"<SESSION_ID>"` | `Session.sessionId`, a fresh `UUID().uuidString` per process launch | `auth/*.json` → `metadata.session_id` |
| `"<TRANSCRIPT_UUID>"` | `transcriptUUID`, a fresh `UUID().uuidString` per dictation | `auth/*.json` → `metadata.transcript_entity_uuid` |
| `"<TOKEN>"` | the raw access token | `polish/headers.json` → `client_set_headers.Authorization` |

The frames are built with the real generated UUIDs and substituted afterwards, so the
builder itself stays a faithful copy of the original expression. A port should assert on
these two fields being *a* UUID, not on their value.

Everything else is fixed by construction: the synthetic PCM is integer-only (no `libm`, which
is not bit-identical across platforms), the fixture database uses literal UUIDs and a pinned
`2025-01-01T00:00:00Z` epoch instead of `UUID()`/`Date()`, and every generated directory is
deleted and rebuilt on each run so a stale file cannot survive a layout change.

---

## What each fixture pins

### `auth/` — 22 permutations × 2 files

`<name>.json` is the frame; `<name>.input.json` is everything needed to reproduce it
(the full `AppSettings`, the session fields, `appInfo`, both contexts, and the dictionary
output), plus a one-line `purpose` describing what that case is for. A golden frame with no
recorded inputs is untestable, hence the pairing.

Coverage: `aiFormatting` on/off (`pipeline`), `styleDetectionEnabled` on/off
(`personalization_style_settings`), `creatorMode` on/off (`job_selectors`),
`commandModeEnabled` on/off, `hyperlinkOn` on/off, `autoCleanupLevel` pass-through, empty vs
populated `ax_context` (the only input that flips `prefix_is_written`), populated
`ocr_context` (which does *not*), one vs three languages, absent vs attached
`DictionaryStore`, all four `AppInfoDetector` types (`other`, `messaging`, `email`, `ai`),
uppercase type normalisation, a non-ASCII app name, an all-on case and an all-off case with
a nil access token (which serialises as `""`, not omitted).

`dictionary-populated` is the interesting one: it is produced by the real `DictionaryStore`
against `db/populated.db`, so it pins the `LIMIT 50` cut-off, the `ORDER BY frequency_used
DESC`, the exclusion of `is_deleted` rows, and the single-element-array wrapping applied to
snippets at the call site.

### `append/`, `pcm/`

`pcm/input-1001.pcm` is 1001 packets of 16-bit little-endian mono PCM at 16 kHz — 640 samples
/ 1280 bytes each, 1,281,280 bytes total. 1001 lands one packet past the second chunk
boundary, so the chunking has an awkward final chunk of size 1 rather than a clean multiple.
`pcm/input-1001.wav` is the same audio behind the 44-byte RIFF header
`AppDelegate.saveAudioToDownloads` writes.

Three packets are pinned to edge cases:

| Packet | Content | Pins |
|---|---|---|
| 0 | digital silence | `volume` exactly `0.0`; ascii85 emits 320 consecutive `z` |
| 500 | `Int16.max` throughout | `32767/32768*10000 = 9999.7` must round **up** to `10000` → `volume` `1.0` |
| 1000 | `Int16.min` throughout | RMS exactly `32768` → `volume` `1.0` |

`pcm/packets.json` lists all 1001 volumes so an RMS implementation can be checked
sample-for-sample. `append/position-0000.json`, `position-0500.json` and `position-1000.json`
are the three `append` frames: 500 / 500 / 1 packets, `position` as a *packet index* (not a
byte offset), `final` true only on the last, `packet_duration` `0.04`, `audio_encoding`
the literal `"wav"` even though the payload is headerless PCM. `append/commit.json` is the
closing frame; `append/expected.json` restates the chunking arithmetic.

### `ascii85/`

`<name>.bin` → `<name>.txt` pairs, described in `ascii85/manifest.json`. The `.txt` files have
**no trailing newline**; they are the exact encoder output.

`empty.bin` and `empty.txt` are **intentionally zero bytes** — the empty input encodes to the
empty string, and padding either file would corrupt the vector. They are the only zero-length
files in the tree.

The pair that catches the classic bug is `zeros-1280` vs `zeros-1282`: a full aligned all-zero
group collapses to `z`, but an all-zero *partial tail* must expand to a run of `!`. Same byte
value, two encodings, decided purely by alignment.

### `polish/`

`request.json` is the body, `headers.json` the request line and headers. `Authorization`
carries the **raw** access token with no `Bearer ` prefix. `Content-Length` is listed
separately under `transport_added_headers` because the URL loading system adds it, not the
client. `request.input.json` records the instruction list; note that `PolishService` turns it
into a `{label: true}` map, so the list order never reaches the wire — which is why the
generator passes an explicit sorted list rather than reading
`settings.activePolishInstructions`, whose `Dictionary` iteration order is seeded per process.

### `settings/`

`default.json` is a freshly-constructed `AppSettings` (32 keys; the three optionals are `nil`
and therefore absent). `full.json` has every one of the 35 stored properties moved off its
default, including those three optionals. The computed `activePolishInstructions` correctly
appears in neither.

### `db/`

`schema.sql` is the verbatim `sqlite_master.sql` after `HistoryStore`, `DictionaryStore`,
`PolishStore` and `NotesStore` have each run their `CREATE TABLE`, in the order `AppDelegate`
constructs them, ordered by object name. Implicit indexes are noted as comments since they
have no DDL of their own.

`populated.db` is a database written by the Swift stores — 4 transcripts, 64 dictionary rows,
3 polish rows, 4 notes — so the Rust side can prove it reads a real Swift-written file.
`db/expected.json` records the row counts and what the three cached `DictionaryStore` getters
return from it.

Rows are inserted with literal SQL rather than through `addEntry`/`addNote`, because those
stamp `UUID()` and `Date()` into every row; the column lists mirror each store's `INSERT`
verbatim. Deliberate traps in the data: `formatted_text` and `asr_text` nulls, embedded single
and double quotes, non-ASCII text, a multi-line snippet value, two soft-deleted rows (one with
the *highest* `frequency_used` in the table, so a query that forgets `is_deleted = 0` fails
loudly), 55 vocabulary terms against a `LIMIT` of 50, unique `frequency_used` per row so the
`ORDER BY` has no ties, and a note whose content exceeds 200 characters so the
`content_preview` truncation is exercised. That note is pure ASCII on purpose:
`String.prefix(200)` counts grapheme clusters, and a fixture should not make the port guess
which unit was meant.

### `provenance.json`

SHA-256 of every reference source the generator links or transcribes from. If a digest under
`transcribed` changes, re-check `tools/fixturegen/ProtocolFrames.swift` against the original
before trusting anything in `auth/`, `append/` or `ascii85/`.
