//! Cross-layer contract: the language picker's codes and this crate's Deepgram
//! translation table must agree.
//!
//! # Why this test exists
//!
//! The language picker (`ui/src/settings/languages.ts`) retains legacy short
//! codes rather than BCP-47; each code's meaning lives in its display label.
//! `wl_providers::deepgram::deepgram_language_tag` translates those codes into
//! Deepgram tags, and several translations depend on the label.
//!
//! `zh` is the dangerous one, and the reason this file is not just a comment:
//!
//! - The picker labels `zh` **"Chinese — Traditional (繁體中文)"**.
//! - Deepgram's bare `zh` means **Simplified** (`zh`, `zh-CN`, `zh-Hans` are its
//!   Simplified tags; Traditional is `zh-TW` / `zh-Hant`).
//! - So the crate maps `zh` → `zh-Hant`, which is right *only* while the picker
//!   keeps calling it Traditional.
//!
//! If that table is ever regenerated from an off-the-shelf language list, `zh`
//! silently becomes Simplified, this crate keeps sending `zh-Hant`, and the user
//! gets a confident transcript in the wrong script. Deepgram returns **HTTP
//! 200**. No status code, no health check and no compiler error catches it —
//! the two files are in different languages and nothing else links them. This
//! test is the only guard.
//!
//! It lives in `tests/` rather than beside the code because it is a product
//! contract spanning the UI and this crate, not a `wl-providers` unit — someone
//! opening `deepgram.rs` should not be surprised to find it reading TypeScript.
//!
//! # It must never skip
//!
//! A missing or unparsable picker is a **failure**, not a reason to pass. A
//! guard that quietly stops guarding is worse than no guard, because it still
//! reads green. If the picker moves, re-point the path below; do not delete the
//! assertions.

use std::path::{Path, PathBuf};

use wl_providers::deepgram::{deepgram_language_tag, language_mode, LanguageMode};

fn picker_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/settings/languages.ts")
}

/// The picker source, or a panic explaining what the reader has to fix.
fn picker_source() -> String {
    let path = picker_path();
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read the language picker at {}: {e}\n\n\
             This is a failure, not a skip. `deepgram_language_tag` maps `zh` -> `zh-Hant` \
             and `zhcn` -> `zh-Hans` purely because that table labels them Traditional and \
             Simplified. If the picker moved, re-point this test; do not delete it.",
            path.display()
        )
    })
}

/// The display label for one picker code, or a panic naming the missing code.
fn label_for(source: &str, code: &str) -> String {
    let needle = format!("code: \"{code}\"");
    let line = source
        .lines()
        .find(|l| l.contains(&needle))
        .unwrap_or_else(|| {
            panic!(
                "no `{code}` entry in {}.\n\n\
                 `deepgram_language_tag` has an arm for `{code}`, so either the picker \
                 renamed it — in which case that arm is now dead and the user's selection \
                 reaches Deepgram untranslated — or the entry was dropped and the arm \
                 should go too.",
                picker_path().display()
            )
        });
    line.trim().to_string()
}

#[test]
fn the_picker_still_calls_zh_traditional_which_is_why_it_maps_to_zh_hant() {
    let source = picker_source();
    let label = label_for(&source, "zh");

    assert!(
        label.contains("Traditional"),
        "the picker no longer calls `zh` Traditional:\n  {label}\n\n\
         Deepgram's bare `zh` means SIMPLIFIED, so `zh` -> `zh-Hant` in \
         deepgram_language_tag is now wrong and every Traditional Chinese dictation \
         returns HTTP 200 with the wrong script. Fix the mapping to match the new \
         meaning before changing this test."
    );
    assert_eq!(deepgram_language_tag("zh"), "zh-Hant");
}

#[test]
fn the_picker_still_calls_zhcn_simplified_which_is_why_it_maps_to_zh_hans() {
    let source = picker_source();
    let label = label_for(&source, "zhcn");

    assert!(
        label.contains("Simplified"),
        "the picker no longer calls `zhcn` Simplified:\n  {label}\n\n\
         `zhcn` -> `zh-Hans` in deepgram_language_tag is now wrong."
    );
    assert_eq!(deepgram_language_tag("zhcn"), "zh-Hans");
}

#[test]
fn every_code_the_crate_translates_still_exists_in_the_picker() {
    let source = picker_source();

    // Each remapped code, with the picker word that establishes its meaning.
    // A code that vanishes leaves a dead arm; a code whose meaning drifts makes
    // the translation wrong. Both matter, so both are asserted.
    let contract = [
        ("engb", "British", "en-GB"),
        ("dech", "Swiss", "de-CH"),
        ("zhcn", "Simplified", "zh-Hans"),
        ("zh", "Traditional", "zh-Hant"),
        ("yue", "Cantonese", "zh-HK"),
        ("hien", "Hinglish", "multi"),
    ];

    for (code, meaning, tag) in contract {
        let label = label_for(&source, code);
        assert!(
            label.contains(meaning),
            "the picker's `{code}` no longer mentions \"{meaning}\":\n  {label}\n\n\
             deepgram_language_tag maps it to `{tag}` on the strength of that word."
        );
        assert_eq!(
            deepgram_language_tag(code),
            tag,
            "`{code}` must still translate to `{tag}`"
        );
    }
}

#[test]
fn the_auto_detect_sentinel_is_still_the_pickers_own_pseudo_code() {
    let source = picker_source();

    assert!(
        source.contains("\"auto\""),
        "the picker no longer uses `auto` as its detect-language sentinel.\n\n\
         `language_mode` special-cases `auto` into detect_language=true. If the \
         sentinel was renamed, that branch is dead and the new value falls through \
         to the single-language arm, which sends it as a literal language tag — the \
         exact bug this mapping was introduced to fix."
    );
    assert_eq!(language_mode(&["auto".to_string()]), LanguageMode::Detect);
}
