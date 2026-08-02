//! SQLite storage for transcripts, dictionary entries, and notes.
//!
//! Existing macOS databases remain readable after upgrading.
//! deliberate improvements sit on top of it:
//!
//! - **DV5** — the Swift app had no indexes at all, so paging history was a
//!   full table scan. The three indexes added here are pure additions; they
//!   change no query result.
//! - **Errors are checked.** `DatabaseManager.exec` discarded every SQLite
//!   return code, and every store's prepare guard turned a failure into an
//!   empty result set. A corrupt or unwritable database therefore looked like
//!   an empty one. Here failures surface as [`DbError`].
//!
//! Schema evolution goes through the `user_version` migration runner below.
//! The Swift app had none: it relied on `CREATE TABLE IF NOT EXISTS`, which
//! silently leaves an old database missing any newly added column.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, MutexGuard};
use rusqlite::Connection;

pub mod dictionary;
pub mod history;
pub mod models;
pub mod notes;

pub use dictionary::{CsvImport, DictionaryStore};
pub use history::HistoryStore;
pub use models::{DictionaryEntry, NewTranscript, NoteEntry, TranscriptEntry};
pub use notes::NotesStore;

/// Anything that can go wrong reaching the database.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("cannot prepare database directory {path}: {source}")]
    Directory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot rename legacy database {from} to {to}: {source}")]
    LegacyRename {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T, E = DbError> = std::result::Result<T, E>;

/// Schema revisions, applied in order. Index `i` upgrades the database to
/// `user_version = i + 1`.
///
/// Migration 1 creates the retained tables. Migration 2 removes the retired
/// AI-polish table from upgraded Swift installations.
const SCHEMA_V1: &str = "
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

CREATE INDEX IF NOT EXISTS idx_transcripts_timestamp ON transcripts(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_notes_modified_at ON notes(modified_at DESC);
CREATE INDEX IF NOT EXISTS idx_dictionary_is_deleted ON dictionary(is_deleted);
";

const SCHEMA_V2: &str = "DROP TABLE IF EXISTS polish;";

const MIGRATIONS: &[&str] = &[SCHEMA_V1, SCHEMA_V2];

/// An open database, shared by all three stores.
///
/// One connection behind a mutex rather than a pool: writes arrive from the
/// transcription thread while the UI reads, and SQLite serialises writers
/// anyway. A pool would add contention handling for a workload that has none.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open the app's database, performing the legacy rename and migrations.
    pub fn open() -> Result<Self> {
        let path = crate::paths::database_file();
        let dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        crate::paths::ensure_dir(&dir)
            .map_err(|source| DbError::Directory { path: dir, source })?;

        let legacy = crate::paths::legacy_database_file();
        rename_legacy(&legacy, &path)?;

        Self::open_at(&path)
    }

    /// Open a database at an explicit path. The parent directory must exist.
    pub fn open_at(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        configure(&conn)?;
        migrate(&mut conn)?;
        tracing::info!(path = %path.display(), "database opened");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// A throwaway database with the full schema, for tests and for callers
    /// that need a store without touching the disk.
    pub fn in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        configure(&conn)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// The schema version this build writes.
    pub fn schema_version() -> i32 {
        MIGRATIONS.len() as i32
    }

    /// The version actually recorded in this database.
    pub fn user_version(&self) -> Result<i32> {
        Ok(self
            .conn
            .lock()
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock()
    }
}

/// Move a pre-2.0 `history.db` into place as `lightning.db`.
///
/// Returns whether a rename happened. An existing `lightning.db` is never
/// overwritten — the legacy file is left orphaned instead, which is what the
/// Swift app did and is the only safe choice when both exist.
fn rename_legacy(legacy: &Path, active: &Path) -> Result<bool> {
    if !legacy.exists() || active.exists() {
        return Ok(false);
    }
    std::fs::rename(legacy, active).map_err(|source| DbError::LegacyRename {
        from: legacy.to_path_buf(),
        to: active.to_path_buf(),
        source,
    })?;
    tracing::info!(
        from = %legacy.display(),
        to = %active.display(),
        "migrated legacy database"
    );
    Ok(true)
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL lets the UI read while a transcript is being written. In-memory
    // databases report "memory" and cannot journal; that is not an error.
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if mode != "wal" && mode != "memory" {
        tracing::warn!(mode = %mode, "WAL journal mode unavailable");
    }
    // A second instance of the app (or a leftover one mid-shutdown) can hold
    // the write lock briefly; block rather than fail the user's dictation.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// Apply every migration the database has not seen yet.
///
/// Each runs in its own transaction together with its version bump, so an
/// interrupted upgrade never leaves a half-applied revision recorded.
fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current > Database::schema_version() {
        // A newer build wrote this file. Refusing to open would strand the
        // user on the older version with no way back. Continue with the schema
        // this build understands without applying older migrations.
        tracing::warn!(
            found = current,
            expected = Database::schema_version(),
            "database was written by a newer version"
        );
        return Ok(());
    }
    for (index, script) in MIGRATIONS.iter().enumerate().skip(current.max(0) as usize) {
        let version = index as i32 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(script)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(version, "applied schema migration");
    }
    Ok(())
}

/// Current time as Unix epoch seconds, the format every timestamp column uses.
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

/// A fresh row id: an uppercase UUID, matching Foundation's
/// `UUID().uuidString` so ids written by either implementation look alike.
pub fn new_id() -> String {
    let mut id = uuid::Uuid::new_v4().to_string();
    id.make_ascii_uppercase();
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `CREATE TABLE` statements copied verbatim out of the Swift stores.
    /// Comparing a database built from these against one built from
    /// [`SCHEMA_V1`] is the actual parity guarantee: a file written by the
    /// Swift app must read back identically here.
    const SWIFT_SCHEMA: &str = "
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
";

    /// (name, declared type, not-null, default, primary-key position)
    type Column = (String, String, bool, Option<String>, i64);

    fn columns(conn: &Connection, table: &str) -> Vec<Column> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("table_info");
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? == 1,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .expect("query table_info");
        rows.map(|r| r.expect("column row")).collect()
    }

    /// Column sets of unique indexes, so a lost or widened UNIQUE constraint
    /// is caught even though `table_info` cannot see it.
    fn unique_index_columns(conn: &Connection, table: &str) -> Vec<Vec<String>> {
        let mut list = conn
            .prepare(&format!("PRAGMA index_list({table})"))
            .expect("index_list");
        let names: Vec<String> = list
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? == 1))
            })
            .expect("query index_list")
            .map(|r| r.expect("index row"))
            .filter(|(_, unique)| *unique)
            .map(|(name, _)| name)
            .collect();

        let mut out: Vec<Vec<String>> = names
            .iter()
            .map(|name| {
                let mut info = conn
                    .prepare(&format!("PRAGMA index_info({name})"))
                    .expect("index_info");
                let cols: Vec<String> = info
                    .query_map([], |row| row.get::<_, Option<String>>(2))
                    .expect("query index_info")
                    .map(|r| r.expect("index column").unwrap_or_default())
                    .collect();
                cols
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn schema_matches_the_swift_schema_column_for_column() {
        let ours = Connection::open_in_memory().expect("open ours");
        ours.execute_batch(SCHEMA_V1).expect("our schema");
        let theirs = Connection::open_in_memory().expect("open theirs");
        theirs.execute_batch(SWIFT_SCHEMA).expect("swift schema");

        for table in ["transcripts", "dictionary", "notes"] {
            assert_eq!(
                columns(&ours, table),
                columns(&theirs, table),
                "{table} columns diverge from the Swift schema"
            );
            assert_eq!(
                unique_index_columns(&ours, table),
                unique_index_columns(&theirs, table),
                "{table} unique constraints diverge from the Swift schema"
            );
        }
    }

    #[test]
    fn dictionary_uniqueness_spans_phrase_and_team_dictionary_id() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(SCHEMA_V1).expect("schema");
        assert_eq!(
            unique_index_columns(&conn, "dictionary"),
            vec![
                // The TEXT primary key's implicit unique index.
                vec!["id".to_string()],
                vec!["phrase".to_string(), "team_dictionary_id".to_string()],
            ],
            "re-adding an existing phrase must keep colliding on the pair, not on phrase alone"
        );
    }

    #[test]
    fn the_added_indexes_exist_alongside_the_original_tables() {
        let db = Database::in_memory().expect("open");
        let conn = db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%' ORDER BY name",
            )
            .expect("prepare");
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        assert_eq!(
            names,
            vec![
                "idx_dictionary_is_deleted",
                "idx_notes_modified_at",
                "idx_transcripts_timestamp"
            ]
        );
    }

    #[test]
    fn a_legacy_history_db_is_renamed_once_and_then_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("history.db");
        let active = dir.path().join("lightning.db");
        std::fs::write(&legacy, b"legacy").expect("write legacy");

        assert!(rename_legacy(&legacy, &active).expect("first rename"));
        assert!(!legacy.exists());
        assert_eq!(std::fs::read(&active).expect("read"), b"legacy");

        // Second call has nothing to move.
        assert!(!rename_legacy(&legacy, &active).expect("second rename"));
        assert_eq!(std::fs::read(&active).expect("read"), b"legacy");
    }

    #[test]
    fn a_legacy_history_db_never_clobbers_an_existing_lightning_db() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("history.db");
        let active = dir.path().join("lightning.db");
        std::fs::write(&legacy, b"legacy").expect("write legacy");
        std::fs::write(&active, b"current").expect("write active");

        assert!(!rename_legacy(&legacy, &active).expect("rename"));
        assert_eq!(std::fs::read(&active).expect("read active"), b"current");
        assert!(legacy.exists(), "legacy file is orphaned, never deleted");
    }

    #[test]
    fn migrations_are_idempotent_and_preserve_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("lightning.db");

        let db = std::sync::Arc::new(Database::open_at(&path).expect("first open"));
        assert_eq!(
            db.user_version().expect("version"),
            Database::schema_version()
        );
        NotesStore::new(db.clone())
            .add_note("kept", "content")
            .expect("add note");
        drop(db);

        let db = std::sync::Arc::new(Database::open_at(&path).expect("second open"));
        assert_eq!(
            db.user_version().expect("version"),
            Database::schema_version()
        );
        let notes = NotesStore::new(db.clone()).notes(100).expect("notes");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "kept");
    }

    #[test]
    fn migrating_a_swift_database_adds_the_indexes_without_disturbing_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("lightning.db");
        {
            // Exactly what the Swift app leaves behind: legacy tables, no
            // indexes, user_version still 0.
            let conn = Connection::open(&path).expect("open raw");
            conn.execute_batch(SWIFT_SCHEMA).expect("swift schema");
            conn.execute(
                "INSERT INTO notes (id, title, content_preview, content, created_at, modified_at) VALUES ('A', 't', 'p', 'c', 1.0, 2.0)",
                [],
            )
            .expect("seed note");
            let version: i32 = conn
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .expect("version");
            assert_eq!(version, 0);
        }

        let db = std::sync::Arc::new(Database::open_at(&path).expect("open"));
        assert_eq!(
            db.user_version().expect("version"),
            Database::schema_version()
        );
        let notes = NotesStore::new(db.clone()).notes(100).expect("notes");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, "A");
        assert!(
            columns(&db.lock(), "polish").is_empty(),
            "the retired polish table must be removed during migration"
        );
        let index_count: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'",
                [],
                |row| row.get(0),
            )
            .expect("count indexes");
        assert_eq!(index_count, 3);
    }

    #[test]
    fn a_database_from_a_future_schema_version_still_opens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("lightning.db");
        {
            let conn = Connection::open(&path).expect("open raw");
            conn.execute_batch(SCHEMA_V1).expect("schema");
            conn.pragma_update(None, "user_version", 99).expect("bump");
        }
        let db = Database::open_at(&path).expect("open");
        assert_eq!(db.user_version().expect("version"), 99);
    }

    #[test]
    fn ids_are_uppercase_uuids_like_foundation_writes() {
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id, id.to_uppercase());
        assert_eq!(id.matches('-').count(), 4);
        assert_ne!(id, new_id());
    }
}
