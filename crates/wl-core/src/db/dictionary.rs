//! The user's dictionary: vocabulary hints, replacement pairs and snippets.
//!
//! One table backs three logically distinct kinds, told apart by `is_snippet`
//! and whether `replacement` is NULL. A vocabulary row that also has a
//! replacement deliberately appears in both [`DictionaryStore::vocabulary_phrases`]
//! and [`DictionaryStore::replacements`] — the backend wants the phrase biased
//! *and* rewritten.
//!
//! **DV6.** The three hot-path queries are memoised, because they run while the
//! user is holding the hotkey down and every millisecond there is audible
//! latency. The Swift caches were plain optionals read from the transcription
//! queue while the settings window mutated them on the main queue — an actual
//! data race. Here they sit behind a [`RwLock`], so the common case (all
//! readers, no writer) still never contends.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use rusqlite::{params, Connection};

use super::models::DictionaryEntry;
use super::{new_id, now_secs, Database, Result};

const COLUMNS: &str = "id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at";

/// Vocabulary phrases sent to the backend per dictation.
///
/// Deepgram Nova 3 accepts up to 500 keyterm tokens, so this is the ceiling
/// before the provider applies its own token budget.
const VOCABULARY_LIMIT: i64 = 500;

/// Source tag for phrases mined out of the user's own corrections.
const SOURCE_AUTO_LEARNED: &str = "user_edits";
/// Source tag for rows the app seeds on first run.
const SOURCE_DEFAULT: &str = "default";
/// Source tag for rows the user typed in the settings window.
const SOURCE_MANUAL: &str = "manual";
/// Source tag for rows from a CSV file.
const SOURCE_CSV: &str = "csv_import";

/// Outcome of a CSV import: rows accepted, and one message per rejected line.
///
/// Crosses IPC verbatim; the field names are the frontend contract.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CsvImport {
    pub imported: usize,
    pub errors: Vec<String>,
}

#[derive(Default)]
struct Caches {
    vocabulary: Option<Vec<String>>,
    replacements: Option<BTreeMap<String, String>>,
    snippets: Option<BTreeMap<String, String>>,
}

pub struct DictionaryStore {
    db: Arc<Database>,
    caches: RwLock<Caches>,
}

impl DictionaryStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            caches: RwLock::new(Caches::default()),
        }
    }

    /// Add a phrase. Returns whether a row was actually inserted.
    ///
    /// `INSERT OR IGNORE` against `UNIQUE(phrase, team_dictionary_id)` makes
    /// re-adding an existing phrase a no-op: the existing replacement is left
    /// alone rather than being overwritten by a later, possibly automatic,
    /// insert.
    pub fn add_entry(
        &self,
        phrase: &str,
        replacement: Option<&str>,
        is_snippet: bool,
        source: &str,
        manual_entry: bool,
    ) -> Result<bool> {
        let inserted = insert(
            &self.db.lock(),
            phrase,
            replacement,
            is_snippet,
            source,
            manual_entry,
        )?;
        // Invalidate even when nothing was inserted: the Swift store did, and
        // an unnecessary reload is cheaper than reasoning about when it isn't.
        self.invalidate();
        Ok(inserted)
    }

    /// Add a phrase the user typed themselves.
    pub fn add_manual(
        &self,
        phrase: &str,
        replacement: Option<&str>,
        is_snippet: bool,
    ) -> Result<bool> {
        self.add_entry(phrase, replacement, is_snippet, SOURCE_MANUAL, true)
    }

    /// Add a proper noun mined from the difference between raw and formatted
    /// text, so the recognizer gets it right next time.
    pub fn add_auto_learned_word(&self, phrase: &str) -> Result<bool> {
        self.add_entry(phrase, None, false, SOURCE_AUTO_LEARNED, false)
    }

    /// Batch form of [`Self::add_auto_learned_word`], run as one transaction:
    /// auto-learn fires right after a dictation, and paying a disk sync per
    /// word there is felt.
    pub fn add_auto_learned_words(&self, phrases: &[String]) -> Result<usize> {
        if phrases.is_empty() {
            return Ok(0);
        }
        let mut inserted = 0;
        {
            let mut conn = self.db.lock();
            let tx = conn.transaction()?;
            for phrase in phrases {
                if insert(&tx, phrase, None, false, SOURCE_AUTO_LEARNED, false)? {
                    inserted += 1;
                }
            }
            tx.commit()?;
        }
        self.invalidate();
        Ok(inserted)
    }

    /// Seed the rows a fresh install starts with.
    pub fn seed_defaults(&self, user_name: Option<&str>) -> Result<()> {
        if let Some(name) = user_name.filter(|n| !n.is_empty()) {
            self.add_entry(name, None, false, SOURCE_DEFAULT, false)?;
        }
        self.add_entry("Wispr Lightning", None, false, SOURCE_DEFAULT, false)?;
        Ok(())
    }

    pub fn update_entry(&self, id: &str, phrase: &str, replacement: Option<&str>) -> Result<()> {
        self.db.lock().execute(
            "UPDATE dictionary SET phrase = ?, replacement = ?, modified_at = ? WHERE id = ?",
            params![phrase, replacement, now_secs(), id],
        )?;
        self.invalidate();
        Ok(())
    }

    /// Hide an entry. The row stays, so it keeps occupying its
    /// `UNIQUE(phrase, …)` slot and a later insert of the same phrase is still
    /// ignored — matching the original behaviour exactly.
    pub fn soft_delete(&self, id: &str) -> Result<()> {
        self.db.lock().execute(
            "UPDATE dictionary SET is_deleted = 1, modified_at = ? WHERE id = ?",
            params![now_secs(), id],
        )?;
        self.invalidate();
        Ok(())
    }

    /// Phrases to bias recognition toward, most-used first. Hot path.
    pub fn vocabulary_phrases(&self) -> Result<Vec<String>> {
        {
            let caches = self.caches.read();
            if let Some(cached) = &caches.vocabulary {
                return Ok(cached.clone());
            }
        }
        let fresh = self.query_vocabulary()?;
        self.caches.write().vocabulary = Some(fresh.clone());
        Ok(fresh)
    }

    /// Spoken form to written form. Hot path.
    pub fn replacements(&self) -> Result<BTreeMap<String, String>> {
        {
            let caches = self.caches.read();
            if let Some(cached) = &caches.replacements {
                return Ok(cached.clone());
            }
        }
        let fresh = self.query_pairs("is_snippet = 0 AND replacement IS NOT NULL")?;
        self.caches.write().replacements = Some(fresh.clone());
        Ok(fresh)
    }

    /// Trigger phrase to expansion. Hot path.
    ///
    /// The WSS protocol wraps each value in a single-element array; that
    /// shaping belongs to the protocol layer, not here.
    pub fn snippets(&self) -> Result<BTreeMap<String, String>> {
        {
            let caches = self.caches.read();
            if let Some(cached) = &caches.snippets {
                return Ok(cached.clone());
            }
        }
        let fresh = self.query_pairs("is_snippet = 1")?;
        self.caches.write().snippets = Some(fresh.clone());
        Ok(fresh)
    }

    /// Populate all three caches, so the first dictation of a session does not
    /// pay for three queries while the user is already speaking.
    pub fn warm_up(&self) -> Result<()> {
        self.vocabulary_phrases()?;
        self.replacements()?;
        self.snippets()?;
        Ok(())
    }

    /// One live entry by id, for refreshing a row the UI just edited.
    ///
    /// A soft-deleted row reads back as `None`: nothing outside this module
    /// should ever see a deleted entry.
    pub fn entry(&self, id: &str) -> Result<Option<DictionaryEntry>> {
        self.find("id = ?", id)
    }

    /// One live entry by phrase.
    ///
    /// `phrase` is globally unique in practice — `team_dictionary_id` is never
    /// bound and always takes its default, so `UNIQUE(phrase,
    /// team_dictionary_id)` degenerates to `UNIQUE(phrase)`. That makes this
    /// the accessor to pair with [`Self::add_entry`], which mints its id
    /// internally and cannot hand one back when the insert was ignored.
    ///
    /// `None` after a successful-looking add means the phrase exists only as a
    /// soft-deleted row, which still occupies the unique slot and silently
    /// swallows the insert. That is worth surfacing to the user.
    pub fn entry_by_phrase(&self, phrase: &str) -> Result<Option<DictionaryEntry>> {
        self.find("phrase = ?", phrase)
    }

    fn find(&self, predicate: &str, value: &str) -> Result<Option<DictionaryEntry>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM dictionary WHERE {predicate} AND is_deleted = 0"
        ))?;
        let mut rows = stmt.query_map(params![value], DictionaryEntry::from_row)?;
        rows.next().transpose().map_err(Into::into)
    }

    /// Every live vocabulary entry (`snippet = false`) or snippet
    /// (`snippet = true`), most recently edited first. For the settings window.
    pub fn entries(&self, snippet: bool) -> Result<Vec<DictionaryEntry>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM dictionary
             WHERE is_snippet = ? AND is_deleted = 0
             ORDER BY modified_at DESC"
        ))?;
        let rows = stmt.query_map(params![snippet], DictionaryEntry::from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Substring search over `phrase` only — replacements are not searched,
    /// because the settings list shows and sorts by phrase.
    pub fn search_entries(&self, query: &str, snippet: bool) -> Result<Vec<DictionaryEntry>> {
        let pattern = format!("%{query}%");
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM dictionary
             WHERE is_snippet = ? AND is_deleted = 0 AND phrase LIKE ?
             ORDER BY modified_at DESC"
        ))?;
        let rows = stmt.query_map(params![snippet, pattern], DictionaryEntry::from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Import a two-column CSV. Unreadable files are reported the same way a
    /// malformed line is, because both are the user picking the wrong file.
    pub fn import_csv_file(&self, path: &Path) -> Result<CsvImport> {
        match std::fs::read_to_string(path) {
            Ok(text) => self.import_csv(&text),
            Err(_) => Ok(CsvImport {
                imported: 0,
                errors: vec!["Failed to read file".to_string()],
            }),
        }
    }

    /// `phrase[,replacement]` per line. A row with a second column becomes a
    /// snippet, never a replacement — that is what the settings window's
    /// importer has always produced and the file format has no third column to
    /// say otherwise.
    pub fn import_csv(&self, text: &str) -> Result<CsvImport> {
        let mut report = CsvImport::default();
        for (index, line) in split_lines(text).into_iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if index == 0 && is_header(trimmed) {
                continue;
            }

            let (raw_phrase, raw_replacement) = match trimmed.split_once(',') {
                Some((phrase, rest)) => (phrase, Some(rest)),
                None => (trimmed, None),
            };
            let phrase = unquote(raw_phrase);
            let replacement = raw_replacement.map(unquote);

            if phrase.is_empty() {
                report
                    .errors
                    .push(format!("Line {}: empty phrase", index + 1));
                continue;
            }

            self.add_entry(phrase, replacement, replacement.is_some(), SOURCE_CSV, true)?;
            // Counted as imported even when the phrase already existed: from
            // the user's point of view the line was accepted.
            report.imported += 1;
        }
        Ok(report)
    }

    fn invalidate(&self) {
        *self.caches.write() = Caches::default();
    }

    fn query_vocabulary(&self) -> Result<Vec<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT phrase FROM dictionary
             WHERE is_snippet = 0 AND is_deleted = 0
             ORDER BY frequency_used DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![VOCABULARY_LIMIT], |row| row.get::<_, String>(0))?;
        let phrases = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        // Saturating means the user has more terms than we send, and the tail
        // is invisibly dropped. Nothing else would ever tell them.
        if phrases.len() as i64 >= VOCABULARY_LIMIT {
            tracing::warn!(
                limit = VOCABULARY_LIMIT,
                "dictionary phrase fetch hit the limit; terms beyond it are not sent"
            );
        }
        Ok(phrases)
    }

    /// `phrase -> replacement` for live rows matching `predicate`. Rows whose
    /// replacement is NULL are skipped rather than mapped to an empty string,
    /// which would rewrite the phrase to nothing.
    fn query_pairs(&self, predicate: &str) -> Result<BTreeMap<String, String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT phrase, replacement FROM dictionary WHERE {predicate} AND is_deleted = 0"
        ))?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (phrase, replacement) = row?;
            if let Some(replacement) = replacement {
                out.insert(phrase, replacement);
            }
        }
        Ok(out)
    }
}

fn insert(
    conn: &Connection,
    phrase: &str,
    replacement: Option<&str>,
    is_snippet: bool,
    source: &str,
    manual_entry: bool,
) -> rusqlite::Result<bool> {
    let now = now_secs();
    let changed = conn.execute(
        "INSERT OR IGNORE INTO dictionary
         (id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at)
         VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
        params![
            new_id(),
            phrase,
            replacement,
            is_snippet,
            manual_entry,
            source,
            now,
            now,
        ],
    )?;
    Ok(changed > 0)
}

fn is_header(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("phrase") || lower.contains("abbreviation")
}

/// Trim surrounding whitespace, then surrounding double quotes — the order the
/// Swift importer used, so `  "a b"  ` yields `a b`.
fn unquote(field: &str) -> &str {
    field.trim().trim_matches('"')
}

/// Split on every Unicode line break, treating CRLF as one break.
///
/// Swift's `components(separatedBy: .newlines)` split CRLF into two, producing
/// a phantom empty line that shifted every subsequent error message's line
/// number. Windows-authored CSVs are the common case here, so the numbers are
/// reported correctly instead.
fn split_lines(text: &str) -> Vec<&str> {
    fn is_break(c: char) -> bool {
        matches!(
            c,
            '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
        )
    }

    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if !is_break(c) {
            continue;
        }
        out.push(&text[start..index]);
        let mut end = index + c.len_utf8();
        if c == '\r' {
            if let Some(&(newline, '\n')) = chars.peek() {
                chars.next();
                end = newline + 1;
            }
        }
        start = end;
    }
    out.push(&text[start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (DictionaryStore, Arc<Database>) {
        let db = Arc::new(Database::in_memory().expect("open"));
        (DictionaryStore::new(db.clone()), db)
    }

    fn id_of(store: &DictionaryStore, phrase: &str) -> String {
        store
            .db
            .lock()
            .query_row(
                "SELECT id FROM dictionary WHERE phrase = ?",
                params![phrase],
                |row| row.get(0),
            )
            .expect("row present")
    }

    fn set_frequency(db: &Database, phrase: &str, frequency: i64) {
        db.lock()
            .execute(
                "UPDATE dictionary SET frequency_used = ? WHERE phrase = ?",
                params![frequency, phrase],
            )
            .expect("set frequency");
    }

    #[test]
    fn a_freshly_added_phrase_can_be_read_back_with_its_minted_id_and_stamps() {
        let (store, _db) = store();
        assert!(store
            .add_manual("omw", Some("on my way"), true)
            .expect("add"));

        let entry = store
            .entry_by_phrase("omw")
            .expect("query")
            .expect("present");
        assert_eq!(entry.replacement.as_deref(), Some("on my way"));
        assert!(entry.is_snippet);
        assert!(entry.manual_entry);
        assert_eq!(entry.source.as_deref(), Some("manual"));
        assert_eq!(entry.frequency_used, 0);
        assert_eq!(entry.created_at, entry.modified_at);
        assert_eq!(entry.id.len(), 36, "the store minted a UUID");

        assert_eq!(store.entry(&entry.id).expect("by id"), Some(entry));
    }

    #[test]
    fn looking_up_an_unknown_or_deleted_entry_yields_nothing() {
        let (store, _db) = store();
        assert!(store.entry("NO-SUCH-ID").expect("by id").is_none());
        assert!(store
            .entry_by_phrase("never added")
            .expect("by phrase")
            .is_none());

        store.add_manual("doomed", None, false).expect("add");
        let id = id_of(&store, "doomed");
        store.soft_delete(&id).expect("delete");
        assert!(store.entry(&id).expect("by id").is_none());
        assert!(
            store
                .entry_by_phrase("doomed")
                .expect("by phrase")
                .is_none(),
            "an add swallowed by a soft-deleted row must not look like a success"
        );
    }

    #[test]
    fn an_import_report_crosses_ipc_as_imported_and_errors() {
        let report = CsvImport {
            imported: 2,
            errors: vec!["Line 3: empty phrase".to_string()],
        };
        let json = serde_json::to_value(&report).expect("serialize");
        assert_eq!(json["imported"], 2);
        assert_eq!(json["errors"][0], "Line 3: empty phrase");
        assert_eq!(
            serde_json::from_value::<CsvImport>(json).expect("deserialize"),
            report
        );
    }

    #[test]
    fn adding_a_duplicate_phrase_is_a_silent_no_op() {
        let (store, db) = store();
        assert!(store.add_manual("kubectl", None, false).expect("first"));
        assert!(
            !store
                .add_manual("kubectl", Some("kube control"), false)
                .expect("second"),
            "the second insert is ignored"
        );

        let (count, replacement): (i64, Option<String>) = db
            .lock()
            .query_row(
                "SELECT COUNT(*), MAX(replacement) FROM dictionary WHERE phrase = 'kubectl'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row");
        assert_eq!(count, 1);
        assert_eq!(
            replacement, None,
            "the existing row keeps its replacement, it is not overwritten"
        );
    }

    #[test]
    fn a_soft_deleted_phrase_still_blocks_re_adding_itself() {
        let (store, _db) = store();
        store.add_manual("ephemeral", None, false).expect("add");
        store
            .soft_delete(&id_of(&store, "ephemeral"))
            .expect("delete");

        assert!(
            !store.add_manual("ephemeral", None, false).expect("re-add"),
            "the deleted row keeps its UNIQUE slot"
        );
    }

    #[test]
    fn soft_deleted_entries_are_excluded_from_every_hot_path_query() {
        let (store, _db) = store();
        store.add_manual("Kubernetes", None, false).expect("vocab");
        store
            .add_manual("btw", Some("by the way"), false)
            .expect("replacement");
        store
            .add_manual("sig", Some("Best, Alice"), true)
            .expect("snippet");

        // Populate the caches first: a stale cache would hide the deletion.
        store.warm_up().expect("warm up");
        for phrase in ["Kubernetes", "btw", "sig"] {
            store.soft_delete(&id_of(&store, phrase)).expect("delete");
        }

        assert!(store.vocabulary_phrases().expect("vocab").is_empty());
        assert!(store.replacements().expect("replacements").is_empty());
        assert!(store.snippets().expect("snippets").is_empty());
    }

    #[test]
    fn soft_deleted_entries_are_excluded_from_listings_and_search() {
        let (store, _db) = store();
        store.add_manual("visible", None, false).expect("a");
        store.add_manual("hidden", None, false).expect("b");
        store.soft_delete(&id_of(&store, "hidden")).expect("delete");

        let phrases: Vec<String> = store
            .entries(false)
            .expect("entries")
            .into_iter()
            .map(|e| e.phrase)
            .collect();
        assert_eq!(phrases, ["visible"]);
        assert!(store
            .search_entries("hidden", false)
            .expect("search")
            .is_empty());
    }

    #[test]
    fn vocabulary_excludes_snippets_and_orders_by_frequency() {
        let (store, db) = store();
        store.add_manual("rare", None, false).expect("rare");
        store.add_manual("common", None, false).expect("common");
        store
            .add_manual("expand", Some("expansion"), true)
            .expect("snippet");
        set_frequency(&db, "common", 10);
        set_frequency(&db, "rare", 1);

        assert_eq!(
            store.vocabulary_phrases().expect("vocab"),
            ["common", "rare"]
        );
    }

    /// Raised from 50: users with a few hundred custom terms were silently
    /// losing every one past the fiftieth on every dictation.
    #[test]
    fn vocabulary_is_capped_at_five_hundred_phrases() {
        let (store, _db) = store();
        // One transaction; 510 separate inserts is a slow test for no gain.
        let phrases: Vec<String> = (0..510).map(|i| format!("phrase{i:04}")).collect();
        store.add_auto_learned_words(&phrases).expect("add");

        assert_eq!(store.vocabulary_phrases().expect("vocab").len(), 500);
    }

    #[test]
    fn a_vocabulary_below_the_cap_is_returned_whole() {
        let (store, _db) = store();
        let phrases: Vec<String> = (0..499).map(|i| format!("phrase{i:04}")).collect();
        store.add_auto_learned_words(&phrases).expect("add");

        assert_eq!(store.vocabulary_phrases().expect("vocab").len(), 499);
    }

    #[test]
    fn a_vocabulary_row_with_a_replacement_appears_in_both_queries() {
        let (store, _db) = store();
        store
            .add_manual("Github", Some("GitHub"), false)
            .expect("add");

        assert_eq!(store.vocabulary_phrases().expect("vocab"), ["Github"]);
        assert_eq!(
            store.replacements().expect("replacements").get("Github"),
            Some(&"GitHub".to_string())
        );
        assert!(store.snippets().expect("snippets").is_empty());
    }

    #[test]
    fn replacements_and_snippets_skip_rows_with_no_replacement_text() {
        let (store, db) = store();
        store.add_manual("plain", None, false).expect("plain");
        // A snippet with a NULL replacement can only arrive from a hand-edited
        // database, and expanding it to nothing would delete the user's words.
        db.lock()
            .execute(
                "INSERT INTO dictionary (id, phrase, is_snippet, created_at, modified_at) VALUES ('X', 'broken', 1, 1.0, 1.0)",
                [],
            )
            .expect("broken row");

        assert!(store.replacements().expect("replacements").is_empty());
        assert!(store.snippets().expect("snippets").is_empty());
    }

    #[test]
    fn hot_path_results_are_memoised_between_calls() {
        let (store, db) = store();
        store.add_manual("first", None, false).expect("add");
        assert_eq!(store.vocabulary_phrases().expect("vocab"), ["first"]);

        // Written behind the store's back, so only a cache miss would see it.
        db.lock()
            .execute(
                "INSERT INTO dictionary (id, phrase, is_snippet, created_at, modified_at) VALUES ('Y', 'second', 0, 1.0, 1.0)",
                [],
            )
            .expect("sneak row");

        assert_eq!(
            store.vocabulary_phrases().expect("vocab"),
            ["first"],
            "the cached result is reused"
        );
    }

    #[test]
    fn adding_an_entry_invalidates_the_caches() {
        let (store, _db) = store();
        store.warm_up().expect("warm up");
        store
            .add_manual("fresh", Some("Fresh"), false)
            .expect("add");

        assert_eq!(store.vocabulary_phrases().expect("vocab"), ["fresh"]);
        assert_eq!(store.replacements().expect("replacements").len(), 1);
    }

    #[test]
    fn updating_an_entry_invalidates_the_caches() {
        let (store, _db) = store();
        store.add_manual("teh", Some("the"), false).expect("add");
        store.warm_up().expect("warm up");

        let id = id_of(&store, "teh");
        store
            .update_entry(&id, "recieve", Some("receive"))
            .expect("update");

        assert_eq!(store.vocabulary_phrases().expect("vocab"), ["recieve"]);
        assert_eq!(
            store.replacements().expect("replacements").get("recieve"),
            Some(&"receive".to_string())
        );
    }

    #[test]
    fn a_batch_of_auto_learned_words_invalidates_the_caches_once_committed() {
        let (store, _db) = store();
        store.warm_up().expect("warm up");
        let words = vec!["Anthropic".to_string(), "Tauri".to_string()];
        assert_eq!(store.add_auto_learned_words(&words).expect("batch"), 2);

        let mut phrases = store.vocabulary_phrases().expect("vocab");
        phrases.sort();
        assert_eq!(phrases, ["Anthropic", "Tauri"]);
    }

    #[test]
    fn an_auto_learned_batch_reports_only_the_rows_it_actually_inserted() {
        let (store, _db) = store();
        store.add_manual("Tauri", None, false).expect("existing");
        let words = vec!["Tauri".to_string(), "Rust".to_string()];
        assert_eq!(store.add_auto_learned_words(&words).expect("batch"), 1);
    }

    #[test]
    fn an_empty_auto_learn_batch_touches_nothing() {
        let (store, _db) = store();
        assert_eq!(store.add_auto_learned_words(&[]).expect("batch"), 0);
    }

    #[test]
    fn auto_learned_words_are_tagged_so_they_can_be_told_from_manual_ones() {
        let (store, _db) = store();
        store.add_auto_learned_word("Deepgram").expect("add");
        let entry = &store.entries(false).expect("entries")[0];
        assert_eq!(entry.source.as_deref(), Some("user_edits"));
        assert!(!entry.manual_entry);
    }

    #[test]
    fn seeding_defaults_skips_an_empty_user_name() {
        let (store, _db) = store();
        store.seed_defaults(Some("")).expect("seed");
        assert_eq!(
            store.vocabulary_phrases().expect("vocab"),
            ["Wispr Lightning"]
        );

        store.seed_defaults(Some("Ada Lovelace")).expect("seed");
        let mut phrases = store.vocabulary_phrases().expect("vocab");
        phrases.sort();
        assert_eq!(phrases, ["Ada Lovelace", "Wispr Lightning"]);
    }

    #[test]
    fn entries_are_listed_most_recently_modified_first() {
        let (store, db) = store();
        store.add_manual("old", None, false).expect("old");
        store.add_manual("new", None, false).expect("new");
        db.lock()
            .execute(
                "UPDATE dictionary SET modified_at = CASE phrase WHEN 'old' THEN 1.0 ELSE 2.0 END",
                [],
            )
            .expect("stamp");

        let phrases: Vec<String> = store
            .entries(false)
            .expect("entries")
            .into_iter()
            .map(|e| e.phrase)
            .collect();
        assert_eq!(phrases, ["new", "old"]);
    }

    #[test]
    fn search_matches_the_phrase_and_not_the_replacement() {
        let (store, _db) = store();
        store
            .add_manual("omw", Some("on my way"), false)
            .expect("add");

        assert_eq!(store.search_entries("omw", false).expect("hit").len(), 1);
        assert!(
            store.search_entries("way", false).expect("miss").is_empty(),
            "replacements are not searched"
        );
    }

    #[test]
    fn csv_import_skips_a_header_row_only_on_the_first_line() {
        let (store, _db) = store();
        let report = store
            .import_csv("Phrase,Replacement\nomw,on my way\nphrase,not a header\n")
            .expect("import");
        assert_eq!(report.imported, 2);
        assert!(report.errors.is_empty());

        let snippets = store.snippets().expect("snippets");
        assert_eq!(snippets.get("omw"), Some(&"on my way".to_string()));
        assert_eq!(snippets.get("phrase"), Some(&"not a header".to_string()));
    }

    #[test]
    fn csv_rows_with_two_columns_become_snippets_and_single_columns_vocabulary() {
        let (store, _db) = store();
        store.import_csv("solo\npair,expanded\n").expect("import");

        assert_eq!(store.vocabulary_phrases().expect("vocab"), ["solo"]);
        assert_eq!(
            store.snippets().expect("snippets").get("pair"),
            Some(&"expanded".to_string())
        );
    }

    #[test]
    fn csv_replacements_keep_their_commas() {
        let (store, _db) = store();
        store
            .import_csv("sig,\"Best regards, Alice\"\n")
            .expect("import");
        assert_eq!(
            store.snippets().expect("snippets").get("sig"),
            Some(&"Best regards, Alice".to_string())
        );
    }

    #[test]
    fn csv_reports_empty_phrases_by_line_number_and_keeps_going() {
        let (store, _db) = store();
        let report = store
            .import_csv("good,one\n,orphan\nalso,fine\n")
            .expect("import");
        assert_eq!(report.imported, 2);
        assert_eq!(report.errors, ["Line 2: empty phrase"]);
    }

    #[test]
    fn csv_line_numbers_are_not_skewed_by_windows_line_endings() {
        let (store, _db) = store();
        let report = store
            .import_csv("phrase,replacement\r\ngood,one\r\n,orphan\r\n")
            .expect("import");
        assert_eq!(report.imported, 1);
        assert_eq!(report.errors, ["Line 3: empty phrase"]);
    }

    #[test]
    fn csv_counts_a_duplicate_line_as_imported_even_though_it_inserts_nothing() {
        let (store, db) = store();
        let report = store
            .import_csv("omw,on my way\nomw,on my way\n")
            .expect("import");
        assert_eq!(report.imported, 2);

        let rows: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM dictionary", [], |row| row.get(0))
            .expect("count");
        assert_eq!(rows, 1);
    }

    #[test]
    fn csv_ignores_blank_lines_without_reporting_them() {
        let (store, _db) = store();
        let report = store.import_csv("solo\n\n   \n\nother\n").expect("import");
        assert_eq!(report.imported, 2);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn an_unreadable_csv_reports_a_single_read_error() {
        let (store, _db) = store();
        let report = store
            .import_csv_file(Path::new("/nonexistent/dictionary.csv"))
            .expect("import");
        assert_eq!(
            report,
            CsvImport {
                imported: 0,
                errors: vec!["Failed to read file".to_string()],
            }
        );
    }

    #[test]
    fn a_csv_file_on_disk_imports_the_same_as_its_text() {
        let (store, _db) = store();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dict.csv");
        std::fs::write(&path, "phrase,replacement\nbrb,be right back\n").expect("write");

        let report = store.import_csv_file(&path).expect("import");
        assert_eq!(report.imported, 1);
        assert_eq!(
            store.snippets().expect("snippets").get("brb"),
            Some(&"be right back".to_string())
        );
    }

    #[test]
    fn split_lines_treats_crlf_as_one_break() {
        assert_eq!(split_lines("a\r\nb\rc\nd"), ["a", "b", "c", "d"]);
        assert_eq!(split_lines("a\u{2028}b"), ["a", "b"]);
        assert_eq!(split_lines(""), [""]);
    }
}
