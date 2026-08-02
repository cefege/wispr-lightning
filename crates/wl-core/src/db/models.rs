//! Row types for the four tables.
//!
//! Timestamps are Unix epoch **seconds** as `f64`, matching what the Swift app
//! wrote (`Date().timeIntervalSince1970`). They are deliberately left as raw
//! epoch values rather than a date type: this crate has no timezone database,
//! and converting at the storage boundary would force a calendar dependency on
//! every consumer.
//!
//! Integer columns are `i64` — SQLite's native integer width — so a value
//! written by another writer can never silently truncate on read.
//!
//! The three read models cross the IPC boundary into the webview, so their
//! serialized field names are a frontend contract; the test at the bottom
//! pins them.

use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// A stored transcript, as read back from `transcripts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub id: String,
    /// Raw recognizer output. NULL in the database when the provider only
    /// returned formatted text.
    pub asr_text: Option<String>,
    pub formatted_text: Option<String>,
    /// Insert time, not recording time — the Swift app bound `now` at INSERT
    /// and history ordering depends on that.
    pub timestamp: f64,
    pub app_name: String,
    pub app_bundle_id: String,
    pub duration_secs: f64,
    pub num_words: i64,
    pub language: String,
}

impl TranscriptEntry {
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            asr_text: row.get(1)?,
            formatted_text: row.get(2)?,
            timestamp: row.get::<_, Option<f64>>(3)?.unwrap_or_default(),
            app_name: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            app_bundle_id: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            duration_secs: row.get::<_, Option<f64>>(6)?.unwrap_or_default(),
            num_words: row.get::<_, Option<i64>>(7)?.unwrap_or_default(),
            // The Swift row mapper substituted "en" for a NULL language, and
            // rows written before the column was populated depend on it.
            language: row
                .get::<_, Option<String>>(8)?
                .unwrap_or_else(|| "en".to_string()),
        })
    }
}

/// A transcript about to be written. The `timestamp` column is supplied by the
/// store, not the caller, so every row is stamped at insert time.
#[derive(Debug, Clone, PartialEq)]
pub struct NewTranscript {
    /// The provider's transcript id, reused across retries so a retried
    /// dictation replaces its earlier row instead of duplicating it.
    pub id: String,
    pub asr_text: Option<String>,
    pub formatted_text: Option<String>,
    pub app_name: String,
    pub app_bundle_id: String,
    pub duration_secs: f64,
    pub num_words: i64,
    pub language: String,
}

/// A dictionary row: vocabulary phrase, replacement pair, or snippet.
///
/// Which of the three it is depends entirely on two columns — `is_snippet` and
/// whether `replacement` is NULL. See `docs/parity/data-spec.md` §4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub phrase: String,
    pub replacement: Option<String>,
    pub is_snippet: bool,
    pub manual_entry: bool,
    pub source: Option<String>,
    pub frequency_used: i64,
    pub created_at: f64,
    pub modified_at: f64,
}

impl DictionaryEntry {
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            phrase: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            replacement: row.get(2)?,
            is_snippet: row.get::<_, Option<i64>>(3)?.unwrap_or_default() == 1,
            manual_entry: row.get::<_, Option<i64>>(4)?.unwrap_or_default() == 1,
            source: row.get(5)?,
            frequency_used: row.get::<_, Option<i64>>(6)?.unwrap_or_default(),
            created_at: row.get::<_, Option<f64>>(7)?.unwrap_or_default(),
            modified_at: row.get::<_, Option<f64>>(8)?.unwrap_or_default(),
        })
    }
}

/// A note. `content_preview` is derived, never supplied by the caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEntry {
    pub id: String,
    pub title: String,
    pub content_preview: String,
    pub content: String,
    pub created_at: f64,
    pub modified_at: f64,
}

impl NoteEntry {
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            content_preview: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            content: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            created_at: row.get::<_, Option<f64>>(4)?.unwrap_or_default(),
            modified_at: row.get::<_, Option<f64>>(5)?.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JSON object key order is not part of the contract — the names are.
    fn keys(value: &serde_json::Value) -> Vec<String> {
        let mut names: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        names.sort();
        names
    }

    fn sorted<const N: usize>(names: [&str; N]) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(|n| (*n).to_string()).collect();
        names.sort();
        names
    }

    #[test]
    fn transcripts_reach_the_webview_under_their_camel_case_names() {
        let entry = TranscriptEntry {
            id: "A".to_string(),
            asr_text: None,
            formatted_text: Some("Hello.".to_string()),
            timestamp: 1.0,
            app_name: "Mail".to_string(),
            app_bundle_id: "com.apple.mail".to_string(),
            duration_secs: 2.0,
            num_words: 1,
            language: "en".to_string(),
        };
        let json = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(
            keys(&json),
            sorted([
                "id",
                "asrText",
                "formattedText",
                "timestamp",
                "appName",
                "appBundleId",
                "durationSecs",
                "numWords",
                "language"
            ])
        );
        assert_eq!(
            serde_json::from_value::<TranscriptEntry>(json).expect("deserialize"),
            entry
        );
    }

    #[test]
    fn dictionary_entries_reach_the_webview_under_their_camel_case_names() {
        let entry = DictionaryEntry {
            id: "B".to_string(),
            phrase: "omw".to_string(),
            replacement: Some("on my way".to_string()),
            is_snippet: true,
            manual_entry: true,
            source: Some("manual".to_string()),
            frequency_used: 0,
            created_at: 1.0,
            modified_at: 2.0,
        };
        let json = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(
            keys(&json),
            sorted([
                "id",
                "phrase",
                "replacement",
                "isSnippet",
                "manualEntry",
                "source",
                "frequencyUsed",
                "createdAt",
                "modifiedAt"
            ])
        );
        assert_eq!(
            serde_json::from_value::<DictionaryEntry>(json).expect("deserialize"),
            entry
        );
    }

    #[test]
    fn notes_reach_the_webview_under_their_camel_case_names() {
        let entry = NoteEntry {
            id: "C".to_string(),
            title: "Title".to_string(),
            content_preview: "Body".to_string(),
            content: "Body".to_string(),
            created_at: 1.0,
            modified_at: 2.0,
        };
        let json = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(
            keys(&json),
            sorted([
                "id",
                "title",
                "contentPreview",
                "content",
                "createdAt",
                "modifiedAt"
            ])
        );
        assert_eq!(
            serde_json::from_value::<NoteEntry>(json).expect("deserialize"),
            entry
        );
    }

    #[test]
    fn a_missing_optional_text_column_serializes_as_null_not_as_an_absent_key() {
        let entry = NoteEntry {
            id: "D".to_string(),
            title: String::new(),
            content_preview: String::new(),
            content: String::new(),
            created_at: 0.0,
            modified_at: 0.0,
        };
        let json = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(json["title"], serde_json::Value::String(String::new()));

        let transcript = TranscriptEntry {
            id: "E".to_string(),
            asr_text: None,
            formatted_text: None,
            timestamp: 0.0,
            app_name: String::new(),
            app_bundle_id: String::new(),
            duration_secs: 0.0,
            num_words: 0,
            language: "en".to_string(),
        };
        let json = serde_json::to_value(&transcript).expect("serialize");
        assert!(
            json.get("asrText").is_some_and(serde_json::Value::is_null),
            "the frontend distinguishes a null transcript field from a missing one"
        );
    }
}
