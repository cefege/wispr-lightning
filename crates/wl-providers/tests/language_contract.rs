//! The Rust language catalog and Deepgram translation table must agree.
//!
//! `zh` is the dangerous entry: the catalog labels it Traditional, while
//! Deepgram's bare `zh` means Simplified. This test guards against that meaning
//! drifting when either the catalog or translation changes.

use wl_providers::deepgram::{deepgram_language_tag, language_mode, LanguageMode};
use wl_providers::languages::LANGUAGES;

/// The display label for one catalog code, or a panic naming the missing code.
fn label_for(code: &str) -> &'static str {
    LANGUAGES
        .iter()
        .find(|language| language.code == code)
        .map(|language| language.name)
        .unwrap_or_else(|| {
            panic!(
                "no `{code}` entry in wl_providers::languages::LANGUAGES.\n\n\
                 `deepgram_language_tag` has an arm for `{code}`, so either the catalog \
                 renamed it — in which case that arm is now dead and the user's selection \
                 reaches Deepgram untranslated — or the entry was dropped and the arm \
                 should go too."
            )
        })
}

#[test]
fn the_picker_still_calls_zh_traditional_which_is_why_it_maps_to_zh_hant() {
    let label = label_for("zh");

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
    let label = label_for("zhcn");

    assert!(
        label.contains("Simplified"),
        "the picker no longer calls `zhcn` Simplified:\n  {label}\n\n\
         `zhcn` -> `zh-Hans` in deepgram_language_tag is now wrong."
    );
    assert_eq!(deepgram_language_tag("zhcn"), "zh-Hans");
}

#[test]
fn every_code_the_crate_translates_still_exists_in_the_picker() {

    // Each remapped code, with the picker word that establishes its meaning.
    // A code that vanishes leaves a dead arm; a code whose meaning drifts makes
    // the translation wrong. Both matter, so both are asserted.
    let contract = [
        ("engb", "British", "en-GB"),
        ("dech", "Swiss", "de-CH"),
        ("zhcn", "Simplified", "zh-Hans"),
        ("zh", "Traditional", "zh-Hant"),
        ("yue", "Cantonese", "zh-HK"),
    ];

    for (code, meaning, tag) in contract {
        let label = label_for(code);
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



/// `hien` ("Hinglish") is deliberately absent from the Rust language catalog.

/// It never selected a Hindi-English mode: it translates to `multi`, the
/// code-switching pseudo-language, which is exactly what Auto-detect sends. It
/// was a third label for one behaviour, so the catalog stopped offering it.

/// The translation stays. Settings files written before it was retired still
/// hold `hien`, and those users keep the behaviour they chose rather than
/// having their language silently reinterpreted as a literal `hien` tag that
/// Deepgram would reject.
#[test]
fn the_retired_hinglish_code_still_translates_for_settings_that_hold_it() {
    assert_eq!(deepgram_language_tag("hien"), "multi");
    assert!(
        !LANGUAGES.iter().any(|language| language.code == "hien"),
        "`hien` is back in the picker: it is indistinguishable from Auto-detect, \
         so either drop it again or give the picker a label that says so."
    );
}
/// `auto` is a legacy settings value, no longer emitted by the Rust UI.

/// It was the shared picker's detect sentinel. Deepgram's own picker uses
/// `__auto__`, so nothing in the UI writes `auto` any more.

/// The mapping stays because settings files predating the cutover still hold
/// it, in `languages` lists that `migrate` folds into `deepgramLanguage`.
/// Without the special case it would fall through to the single-language arm
/// and be sent as a literal `auto` tag, which Deepgram rejects — the exact bug
/// this mapping was introduced to fix.
#[test]
fn the_legacy_shared_auto_sentinel_still_means_detect() {
    assert_eq!(language_mode(&["auto".to_string()]), LanguageMode::Detect);
    assert_eq!(
        language_mode(&["auto".to_string(), "de".to_string()]),
        LanguageMode::Detect,
        "detection must win regardless of what else the legacy list held"
    );
}
