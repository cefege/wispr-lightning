use std::collections::BTreeMap;

use chrono::{DateTime, Local, NaiveDate};
use wl_core::db::models::TranscriptEntry;
use wl_providers::languages::{AUTO_DETECT, LANGUAGES, LEGACY_MULTI};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageOption {
    pub value: &'static str,
    pub label: String,
}

/// Group newest-first by local calendar day, with newest entries first per day.
pub fn group_by_day(
    entries: Vec<TranscriptEntry>,
    now: DateTime<Local>,
) -> Vec<(String, Vec<TranscriptEntry>)> {
    let today = now.date_naive();
    let yesterday = today.pred_opt();
    let mut buckets = BTreeMap::<NaiveDate, Vec<TranscriptEntry>>::new();

    for entry in entries {
        if let Some(at) = DateTime::from_timestamp_millis((entry.timestamp * 1000.0) as i64) {
            buckets
                .entry(at.with_timezone(&Local).date_naive())
                .or_default()
                .push(entry);
        }
    }

    buckets
        .into_iter()
        .rev()
        .map(|(date, mut entries)| {
            entries.sort_by(|a, b| b.timestamp.total_cmp(&a.timestamp));
            let title = if date == today {
                "Today".to_owned()
            } else if Some(date) == yesterday {
                "Yesterday".to_owned()
            } else {
                date.format("%b %-d").to_string()
            };
            (title, entries)
        })
        .collect()
}

/// Render Deepgram's balance using the unit-specific wording used by the UI.
pub fn balance(amount: f64, units: &str) -> String {
    match units {
        "usd" => format!("${:.2}", (amount * 100.0).round() / 100.0),
        "hour" => format!("{:.1} hours", (amount * 10.0).round() / 10.0),
        _ => format!("{amount} {units}"),
    }
}

/// Return the advisory for a phrase that Deepgram cannot use as a keyterm.
pub fn keyterm_warning(phrase: &str) -> Option<&'static str> {
    let trimmed = phrase.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains(',') || trimmed.contains(';') {
        Some("Deepgram splits keyterms on commas and semicolons, so this phrase will not be sent as a recognition hint. A local replacement still applies.")
    } else if trailing_intensifier(trimmed) {
        Some("Deepgram reads a trailing “:number” as a boost level, so only the text before the colon can be used as a recognition hint. A local replacement still applies.")
    } else {
        None
    }
}

fn trailing_intensifier(value: &str) -> bool {
    let Some((_, digits)) = value.rsplit_once(':') else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Build the language picker rows, preserving a stored Nova-3-only choice on
/// Nova 2 so the UI can explain why it is unsupported.
pub fn language_options(model: &str, stored: &str) -> Vec<LanguageOption> {
    let supports_nova2 = model.to_lowercase().starts_with("nova-2");
    let selected = if stored == LEGACY_MULTI {
        AUTO_DETECT
    } else {
        stored
    };
    let mut options: Vec<_> = LANGUAGES
        .iter()
        .filter(|language| !supports_nova2 || language.nova2)
        .map(|language| LanguageOption {
            value: language.code,
            label: language.name.to_owned(),
        })
        .collect();

    let unsupported =
        selected != AUTO_DETECT && !options.iter().any(|option| option.value == selected);
    if unsupported {
        if let Some(language) = LANGUAGES.iter().find(|language| language.code == selected) {
            options.push(LanguageOption {
                value: language.code,
                label: format!("{} — not available on Nova 2", language.name),
            });
        }
    }

    let mut rows = Vec::with_capacity(options.len() + 1);
    rows.push(LanguageOption {
        value: AUTO_DETECT,
        label: "Auto-detect (multilingual)".to_owned(),
    });
    rows.extend(options.into_iter().filter(|option| option.value != AUTO_DETECT));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn entry(timestamp: f64, id: &str) -> TranscriptEntry {
        TranscriptEntry {
            id: id.to_owned(),
            asr_text: None,
            formatted_text: None,
            timestamp,
            app_name: String::new(),
            app_bundle_id: String::new(),
            duration_secs: 0.0,
            num_words: 0,
            language: "en".to_owned(),
        }
    }

    fn local_time(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn group_by_day_uses_local_midnight_yesterday_and_older_titles() {
        let now = local_time(2026, 3, 5, 0, 5);
        let entries = vec![
            entry(local_time(2026, 3, 5, 0, 1).timestamp() as f64, "today"),
            entry(local_time(2026, 3, 4, 23, 59).timestamp() as f64, "yesterday"),
            entry(local_time(2026, 3, 3, 12, 0).timestamp() as f64, "older"),
        ];
        let groups = group_by_day(entries, now);
        assert_eq!(
            groups.iter().map(|(title, _)| title.as_str()).collect::<Vec<_>>(),
            ["Today", "Yesterday", "Mar 3"]
        );
    }

    #[test]
    fn group_by_day_sorts_entries_newest_first_within_each_group() {
        let now = local_time(2026, 3, 5, 12, 0);
        let first = local_time(2026, 3, 5, 9, 0).timestamp() as f64;
        let later = local_time(2026, 3, 5, 11, 0).timestamp() as f64;
        let groups = group_by_day(vec![entry(first, "first"), entry(later, "later")], now);
        assert_eq!(
            groups[0].1.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(),
            ["later", "first"]
        );
    }

    #[test]
    fn keyterm_warning_matches_punctuation_and_intensifier_boundaries() {
        assert!(keyterm_warning("   ").is_none());
        assert!(keyterm_warning("clean phrase").is_none());
        assert!(keyterm_warning("acme, inc").unwrap().contains("commas and semicolons"));
        assert!(keyterm_warning("acme; inc").unwrap().contains("commas and semicolons"));
        assert!(keyterm_warning("plan:5").unwrap().contains("trailing “:number”"));
        assert!(keyterm_warning(":55").is_some());
        assert!(keyterm_warning("plan:").is_none());
        assert!(keyterm_warning("plan:5x").is_none());
    }

    #[test]
    fn balance_formats_usd_hours_and_other_units() {
        assert_eq!(balance(1.2, "usd"), "$1.20");
        assert_eq!(balance(1.125, "usd"), "$1.13");
        assert_eq!(balance(2.25, "hour"), "2.3 hours");
        assert_eq!(balance(4.5, "credit"), "4.5 credit");
    }

    #[test]
    fn nova_two_keeps_unsupported_stored_language_with_warning_label() {
        let options = language_options("nova-2", "ar");
        assert_eq!(options[0].value, AUTO_DETECT);
        assert_eq!(
            options.iter().find(|option| option.value == "ar").unwrap().label,
            "Arabic (العربية) — not available on Nova 2"
        );
        assert!(!LANGUAGES.iter().find(|language| language.code == "ar").unwrap().nova2);
    }
}
