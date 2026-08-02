//! Long-form notes dictated into the app's own editor.

use std::sync::Arc;

use rusqlite::params;

use super::models::NoteEntry;
use super::{new_id, now_secs, Database, Result};

const COLUMNS: &str = "id, title, content_preview, content, created_at, modified_at";

/// Characters kept in `content_preview` for the list view.
///
/// Swift counted extended grapheme clusters; this counts `char`s (Unicode
/// scalar values). The two differ only for combining sequences and emoji
/// sequences straddling the boundary, where the preview ends up a few code
/// points shorter or longer than Swift's — never truncated mid-scalar, and
/// never visible in the list, which elides the text anyway.
const PREVIEW_CHARS: usize = 200;

/// Rows returned by a search, matching the Swift cap.
const SEARCH_LIMIT: i64 = 100;

pub struct NotesStore {
    db: Arc<Database>,
}

impl NotesStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Create a note and return its id.
    pub fn add_note(&self, title: &str, content: &str) -> Result<String> {
        let id = new_id();
        let now = now_secs();
        self.db.lock().execute(
            "INSERT INTO notes (id, title, content_preview, content, created_at, modified_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![id, title, preview(content), content, now, now],
        )?;
        Ok(id)
    }

    /// Overwrite a note's title and body. The preview is always recomputed, so
    /// it can never drift from the content it summarises.
    pub fn update_note(&self, id: &str, title: &str, content: &str) -> Result<()> {
        self.db.lock().execute(
            "UPDATE notes SET title = ?, content_preview = ?, content = ?, modified_at = ? WHERE id = ?",
            params![title, preview(content), content, now_secs(), id],
        )?;
        Ok(())
    }

    /// Hide a note. Rows are kept so a future sync can propagate the deletion.
    pub fn soft_delete(&self, id: &str) -> Result<()> {
        self.db.lock().execute(
            "UPDATE notes SET is_deleted = 1, modified_at = ? WHERE id = ?",
            params![now_secs(), id],
        )?;
        Ok(())
    }

    /// One live note by id.
    ///
    /// Exists so a caller that just created or edited a note can read back the
    /// persisted row rather than reconstructing it — in particular
    /// `content_preview`, which is derived here and must not be re-derived
    /// elsewhere. A soft-deleted note reads back as `None`.
    pub fn note(&self, id: &str) -> Result<Option<NoteEntry>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM notes WHERE id = ? AND is_deleted = 0"
        ))?;
        let mut rows = stmt.query_map(params![id], NoteEntry::from_row)?;
        rows.next().transpose().map_err(Into::into)
    }

    /// Most recently edited notes first.
    pub fn notes(&self, limit: i64) -> Result<Vec<NoteEntry>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM notes WHERE is_deleted = 0 ORDER BY modified_at DESC LIMIT ?"
        ))?;
        let rows = stmt.query_map(params![limit], NoteEntry::from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Search titles and full bodies — not the truncated preview, so a match
    /// past the first 200 characters is still found.
    pub fn search(&self, query: &str) -> Result<Vec<NoteEntry>> {
        let pattern = format!("%{query}%");
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM notes
             WHERE is_deleted = 0 AND (title LIKE ? OR content LIKE ?)
             ORDER BY modified_at DESC LIMIT ?"
        ))?;
        let rows = stmt.query_map(params![pattern, pattern, SEARCH_LIMIT], NoteEntry::from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn preview(content: &str) -> String {
    content.chars().take(PREVIEW_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> NotesStore {
        NotesStore::new(Arc::new(Database::in_memory().expect("open")))
    }

    fn fetch(store: &NotesStore, id: &str) -> NoteEntry {
        store
            .notes(100)
            .expect("notes")
            .into_iter()
            .find(|n| n.id == id)
            .expect("note present")
    }

    #[test]
    fn a_note_can_be_read_back_by_the_id_its_creation_returned() {
        let store = store();
        let id = store.add_note("Title", &"w".repeat(300)).expect("add");

        let note = store.note(&id).expect("query").expect("present");
        assert_eq!(note.id, id);
        assert_eq!(note.title, "Title");
        assert_eq!(
            note.content_preview,
            "w".repeat(200),
            "the caller gets the stored preview rather than re-deriving it"
        );
    }

    #[test]
    fn reading_an_unknown_or_deleted_note_yields_nothing() {
        let store = store();
        assert!(store.note("NO-SUCH-ID").expect("query").is_none());

        let id = store.add_note("Doomed", "body").expect("add");
        store.soft_delete(&id).expect("delete");
        assert!(store.note(&id).expect("query").is_none());
    }

    #[test]
    fn a_new_note_stores_its_content_and_a_preview_of_it() {
        let store = store();
        let id = store.add_note("Title", "Body text").expect("add");
        let note = fetch(&store, &id);
        assert_eq!(note.title, "Title");
        assert_eq!(note.content, "Body text");
        assert_eq!(note.content_preview, "Body text");
        assert_eq!(note.created_at, note.modified_at);
    }

    #[test]
    fn the_preview_truncates_at_two_hundred_characters() {
        let store = store();
        let long = "x".repeat(500);
        let id = store.add_note("Long", &long).expect("add");
        let note = fetch(&store, &id);
        assert_eq!(note.content_preview.chars().count(), 200);
        assert_eq!(note.content.chars().count(), 500);
    }

    #[test]
    fn the_preview_is_recomputed_on_update_rather_than_left_stale() {
        let store = store();
        let id = store.add_note("Title", "short").expect("add");
        let long = "y".repeat(300);
        store.update_note(&id, "Title", &long).expect("update");

        let note = fetch(&store, &id);
        assert_eq!(note.content_preview, "y".repeat(200));
        assert_eq!(note.content_preview.chars().count(), 200);
    }

    #[test]
    fn shortening_a_note_shrinks_its_preview() {
        let store = store();
        let id = store.add_note("Title", &"z".repeat(400)).expect("add");
        store.update_note(&id, "Title", "tiny").expect("update");
        assert_eq!(fetch(&store, &id).content_preview, "tiny");
    }

    #[test]
    fn the_preview_never_splits_a_multi_byte_character() {
        let store = store();
        // 250 three-byte characters: a byte-based cut would corrupt one.
        let content = "é".repeat(250);
        let id = store.add_note("Accents", &content).expect("add");
        let note = fetch(&store, &id);
        assert_eq!(note.content_preview.chars().count(), 200);
        assert!(content.starts_with(&note.content_preview));
    }

    #[test]
    fn notes_are_listed_most_recently_modified_first() {
        let store = store();
        let first = store.add_note("first", "a").expect("first");
        let second = store.add_note("second", "b").expect("second");
        // Touching the older note must float it to the top.
        store.update_note(&first, "first", "edited").expect("edit");

        let ids: Vec<String> = store
            .notes(100)
            .expect("notes")
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids, [first, second]);
    }

    #[test]
    fn the_list_limit_is_honoured() {
        let store = store();
        for i in 0..5 {
            store.add_note(&format!("n{i}"), "x").expect("add");
        }
        assert_eq!(store.notes(2).expect("notes").len(), 2);
    }

    #[test]
    fn a_soft_deleted_note_disappears_from_listing_and_search() {
        let store = store();
        let id = store.add_note("Secret", "hidden body").expect("add");
        store.soft_delete(&id).expect("delete");

        assert!(store.notes(100).expect("notes").is_empty());
        assert!(store.search("hidden").expect("search").is_empty());

        let remaining: i64 = store
            .db
            .lock()
            .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))
            .expect("count");
        assert_eq!(remaining, 1, "the row is retained, only flagged");
    }

    #[test]
    fn search_finds_body_text_beyond_the_preview_window() {
        let store = store();
        let content = format!("{}needle", "a".repeat(400));
        let id = store.add_note("Title", &content).expect("add");

        let hits = store.search("needle").expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
    }

    #[test]
    fn search_matches_titles_too() {
        let store = store();
        store.add_note("Grocery list", "milk").expect("add");
        assert_eq!(store.search("Grocery").expect("search").len(), 1);
    }
}
