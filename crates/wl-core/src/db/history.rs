//! Dictation history.

use std::sync::Arc;

use rusqlite::params;

use super::models::{NewTranscript, TranscriptEntry};
use super::{now_secs, Database, Result};

/// Columns in declaration order. The Swift store used `SELECT *` and indexed
/// the result positionally; naming them makes the same mapping independent of
/// any future column added to the table.
const COLUMNS: &str =
    "id, asr_text, formatted_text, timestamp, app_name, app_bundle_id, duration, num_words, language";

/// Rows returned by a search, matching the Swift cap.
const SEARCH_LIMIT: i64 = 100;

/// Rows older than this are dropped at launch. Six months of history is more
/// than any user browses and already tens of thousands of rows for a heavy one.
const PRUNE_MAX_AGE_SECS: f64 = 180.0 * 24.0 * 60.0 * 60.0;

/// Hard row cap applied after the age cut, so a very heavy user inside the
/// 180-day window still cannot grow the table without bound.
const PRUNE_MAX_ROWS: i64 = 10_000;

pub struct HistoryStore {
    db: Arc<Database>,
}

impl HistoryStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Record a completed dictation, stamped with the current time.
    ///
    /// `INSERT OR REPLACE` keyed on the provider's transcript id: a retry that
    /// reuses the same id updates its row instead of duplicating the entry.
    pub fn add_entry(&self, entry: &NewTranscript) -> Result<()> {
        self.add_entry_at(entry, now_secs())
    }

    pub(crate) fn add_entry_at(&self, entry: &NewTranscript, timestamp: f64) -> Result<()> {
        self.db.lock().execute(
            "INSERT OR REPLACE INTO transcripts
             (id, asr_text, formatted_text, timestamp, app_name, app_bundle_id, duration, num_words, language)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                entry.id,
                entry.asr_text,
                entry.formatted_text,
                timestamp,
                entry.app_name,
                entry.app_bundle_id,
                entry.duration_secs,
                entry.num_words,
                entry.language,
            ],
        )?;
        Ok(())
    }

    /// One page of history, newest first.
    pub fn entries(&self, limit: i64, offset: i64) -> Result<Vec<TranscriptEntry>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM transcripts ORDER BY timestamp DESC LIMIT ? OFFSET ?"
        ))?;
        let rows = stmt.query_map(params![limit, offset], TranscriptEntry::from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Substring search across both the formatted and the raw text.
    ///
    /// `%` and `_` in `query` are deliberately not escaped: the Swift app
    /// honoured them as wildcards and users rely on it.
    pub fn search(&self, query: &str) -> Result<Vec<TranscriptEntry>> {
        let pattern = format!("%{query}%");
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM transcripts
             WHERE formatted_text LIKE ? OR asr_text LIKE ?
             ORDER BY timestamp DESC LIMIT ?"
        ))?;
        let rows = stmt.query_map(
            params![pattern, pattern, SEARCH_LIMIT],
            TranscriptEntry::from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Hard-delete one entry. History is the user's record of what they said;
    /// deleting means deleting.
    pub fn delete_entry(&self, id: &str) -> Result<()> {
        self.db
            .lock()
            .execute("DELETE FROM transcripts WHERE id = ?", params![id])?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        self.db.lock().execute("DELETE FROM transcripts", [])?;
        Ok(())
    }

    /// Drop history outside either retention limit: older than 180 days, or
    /// beyond the newest 10,000 rows.
    ///
    /// Returns the number of rows deleted. The count and the error both matter
    /// to the caller: a wedged database that silently skipped this every
    /// launch is exactly how the table grew unbounded before, so the launch
    /// path logs whatever comes back rather than discarding it.
    pub fn prune(&self) -> Result<usize> {
        self.prune_at(now_secs())
    }

    /// [`Self::prune`] against an explicit clock, so retention is testable
    /// without waiting six months.
    pub(crate) fn prune_at(&self, now: f64) -> Result<usize> {
        let conn = self.db.lock();
        let by_age = conn.execute(
            "DELETE FROM transcripts WHERE timestamp < ?",
            params![now - PRUNE_MAX_AGE_SECS],
        )?;
        // Age first, then the cap, so the cap is spent on rows the user might
        // plausibly still want rather than on ones already due for deletion.
        let by_count = conn.execute(
            "DELETE FROM transcripts WHERE id NOT IN
             (SELECT id FROM transcripts ORDER BY timestamp DESC LIMIT ?)",
            params![PRUNE_MAX_ROWS],
        )?;
        Ok(by_age + by_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> HistoryStore {
        HistoryStore::new(Arc::new(Database::in_memory().expect("open")))
    }

    fn transcript(id: &str, text: &str, words: i64) -> NewTranscript {
        NewTranscript {
            id: id.to_string(),
            asr_text: Some(text.to_string()),
            formatted_text: Some(text.to_string()),
            app_name: "Mail".to_string(),
            app_bundle_id: "com.apple.mail".to_string(),
            duration_secs: 1.5,
            num_words: words,
            language: "en".to_string(),
        }
    }

    #[test]
    fn entries_come_back_newest_first_regardless_of_insertion_order() {
        let store = store();
        store
            .add_entry_at(&transcript("OLD", "old", 1), 100.0)
            .expect("old");
        store
            .add_entry_at(&transcript("NEW", "new", 1), 300.0)
            .expect("new");
        store
            .add_entry_at(&transcript("MID", "mid", 1), 200.0)
            .expect("mid");

        let ids: Vec<String> = store
            .entries(100, 0)
            .expect("entries")
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["NEW", "MID", "OLD"]);
    }

    #[test]
    fn paging_uses_limit_and_offset_against_the_same_ordering() {
        let store = store();
        for i in 0..5 {
            store
                .add_entry_at(&transcript(&format!("E{i}"), "x", 1), f64::from(i))
                .expect("add");
        }
        let page = store.entries(2, 1).expect("page");
        let ids: Vec<&str> = page.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["E3", "E2"]);
    }

    #[test]
    fn search_is_capped_at_one_hundred_rows() {
        let store = store();
        for i in 0..150 {
            store
                .add_entry_at(
                    &transcript(&format!("E{i}"), "needle here", 1),
                    f64::from(i),
                )
                .expect("add");
        }
        assert_eq!(store.search("needle").expect("search").len(), 100);
    }

    #[test]
    fn search_matches_the_raw_text_when_the_formatted_text_does_not() {
        let store = store();
        let mut entry = transcript("A", "ignored", 1);
        entry.asr_text = Some("cromulent".to_string());
        entry.formatted_text = Some("nothing relevant".to_string());
        store.add_entry_at(&entry, 1.0).expect("add");

        let hits = store.search("cromulent").expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "A");
    }

    #[test]
    fn search_returns_matches_newest_first() {
        let store = store();
        store
            .add_entry_at(&transcript("OLD", "hello world", 2), 10.0)
            .expect("old");
        store
            .add_entry_at(&transcript("NEW", "hello again", 2), 20.0)
            .expect("new");
        let ids: Vec<String> = store
            .search("hello")
            .expect("search")
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["NEW", "OLD"]);
    }

    #[test]
    fn re_adding_the_same_transcript_id_replaces_rather_than_duplicates() {
        let store = store();
        store
            .add_entry_at(&transcript("A", "first", 1), 1.0)
            .expect("first");
        store
            .add_entry_at(&transcript("A", "second", 9), 2.0)
            .expect("second");

        let entries = store.entries(100, 0).expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].formatted_text.as_deref(), Some("second"));
        assert_eq!(entries[0].num_words, 9);
    }

    #[test]
    fn deleting_removes_only_the_named_entry() {
        let store = store();
        store
            .add_entry_at(&transcript("A", "a", 1), 1.0)
            .expect("a");
        store
            .add_entry_at(&transcript("B", "b", 1), 2.0)
            .expect("b");

        store.delete_entry("A").expect("delete");
        let ids: Vec<String> = store
            .entries(100, 0)
            .expect("entries")
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["B"]);
    }

    #[test]
    fn clear_all_empties_the_table() {
        let store = store();
        store
            .add_entry_at(&transcript("A", "a", 1), 1.0)
            .expect("a");
        store.clear_all().expect("clear");
        assert!(store.entries(100, 0).expect("entries").is_empty());
    }

    #[test]
    fn a_row_without_a_language_reads_back_as_english() {
        let store = store();
        store
            .db
            .lock()
            .execute(
                "INSERT INTO transcripts (id, timestamp) VALUES ('LEGACY', 1.0)",
                [],
            )
            .expect("legacy row");

        let entries = store.entries(100, 0).expect("entries");
        assert_eq!(entries[0].language, "en");
        assert_eq!(entries[0].app_name, "");
        assert!(entries[0].asr_text.is_none());
    }

    /// One day, in the seconds the `timestamp` column stores.
    const DAY: f64 = 24.0 * 60.0 * 60.0;

    #[test]
    fn prune_keeps_history_inside_the_age_limit_and_drops_the_rest() {
        let store = store();
        let now = 1_800_000_000.0;
        // Straddle the boundary in both directions, one second either side, so
        // an off-by-one in the cutoff cannot pass.
        store
            .add_entry_at(&transcript("YESTERDAY", "x", 1), now - DAY)
            .expect("add");
        store
            .add_entry_at(&transcript("JUST_INSIDE", "x", 1), now - 180.0 * DAY + 1.0)
            .expect("add");
        store
            .add_entry_at(&transcript("ON_THE_LINE", "x", 1), now - 180.0 * DAY)
            .expect("add");
        store
            .add_entry_at(&transcript("JUST_OUTSIDE", "x", 1), now - 180.0 * DAY - 1.0)
            .expect("add");
        store
            .add_entry_at(&transcript("ANCIENT", "x", 1), now - 900.0 * DAY)
            .expect("add");

        assert_eq!(store.prune_at(now).expect("prune"), 2);

        let mut ids: Vec<String> = store
            .entries(100, 0)
            .expect("entries")
            .into_iter()
            .map(|e| e.id)
            .collect();
        ids.sort();
        assert_eq!(ids, ["JUST_INSIDE", "ON_THE_LINE", "YESTERDAY"]);
    }

    #[test]
    fn prune_caps_the_table_at_ten_thousand_newest_rows() {
        let store = store();
        let now = 1_800_000_000.0;
        let rows = PRUNE_MAX_ROWS + 3;
        {
            // One transaction: 10,003 individual commits is a slow test for no
            // extra coverage.
            let mut conn = store.db.lock();
            let tx = conn.transaction().expect("begin");
            for i in 0..rows {
                tx.execute(
                    "INSERT INTO transcripts (id, timestamp) VALUES (?, ?)",
                    params![format!("E{i:05}"), now - (rows - i) as f64],
                )
                .expect("insert");
            }
            tx.commit().expect("commit");
        }

        assert_eq!(store.prune_at(now).expect("prune"), 3);

        let kept = store.entries(PRUNE_MAX_ROWS + 10, 0).expect("entries");
        assert_eq!(kept.len() as i64, PRUNE_MAX_ROWS);
        assert_eq!(kept[0].id, format!("E{:05}", rows - 1), "newest survives");
        assert_eq!(
            kept[kept.len() - 1].id,
            format!("E{:05}", 3),
            "the three oldest are the ones that go"
        );
    }

    #[test]
    fn pruning_an_empty_history_is_a_no_op_rather_than_an_error() {
        let store = store();
        assert_eq!(store.prune_at(1_800_000_000.0).expect("prune"), 0);
        assert!(store.entries(10, 0).expect("entries").is_empty());
    }

    #[test]
    fn the_retention_limits_are_the_documented_ones() {
        // Both are user-visible promises about how much history survives, and
        // both are trivially fat-fingered.
        assert_eq!(PRUNE_MAX_AGE_SECS, 180.0 * DAY);
        assert_eq!(PRUNE_MAX_ROWS, 10_000);
    }
}
