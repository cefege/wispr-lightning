//! Deterministic local cleanup for Deepgram's recognizer output.
//!
//! Replacements and snippets are applied locally because Deepgram receives
//! them only as recognition hints. Capitalization and terminal punctuation
//! then normalize the final text without another network request.

use wl_core::text::{apply_replacements, expand_snippets};

use crate::DictionaryContext;

/// Stages in the local transcript pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostProcessOptions {
    /// Master switch retained for focused unit tests; production uses
    /// [`Self::default`].
    pub enabled: bool,
    /// Uppercase the first letter of the text and of each following sentence.
    pub capitalize_sentences: bool,
    /// Terminate the text with `.` when it ends on a word character.
    pub trailing_punctuation: bool,
}

impl Default for PostProcessOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            capitalize_sentences: true,
            trailing_punctuation: true,
        }
    }
}

impl PostProcessOptions {
    pub const DISABLED: Self = Self {
        enabled: false,
        capitalize_sentences: false,
        trailing_punctuation: false,
    };
}

/// Apply the local pipeline to raw recognizer output.
///
/// Stage order is load-bearing and matches `PORT_PLAN.md` §3.2: replacements
/// first, then snippets. Running snippets first would let an expansion's body
/// be rewritten by a replacement rule the user only meant to apply to their
/// own speech.
///
/// `opts.enabled` is deliberately *not* consulted here — this is the pipeline,
/// not the policy. Use [`format_if_enabled`] when the master switch matters.
pub fn format_locally(asr: &str, dict: &DictionaryContext, opts: &PostProcessOptions) -> String {
    let mut text = apply_replacements(asr, &dict.replacements);
    text = expand_snippets(&text, &dict.snippets);

    if opts.capitalize_sentences {
        text = capitalize_sentences(&text);
    }
    if opts.trailing_punctuation {
        terminate_sentence(&mut text);
    }
    text
}

/// [`format_locally`], gated on the master switch.
///
/// `None` means "publish the raw ASR", which the transcription pipeline
/// represents as `TranscriptResult::formatted_text == None` rather than as a
/// formatted string that happens to equal the raw one — the distinction is
/// what auto-learn uses to tell a correction from a no-op.
pub fn format_if_enabled(
    asr: &str,
    dict: &DictionaryContext,
    opts: &PostProcessOptions,
) -> Option<String> {
    opts.enabled.then(|| format_locally(asr, dict, opts))
}

/// Uppercase the first letter of every sentence.
///
/// A sentence starts at the beginning of the text, after a newline, or after
/// `.`/`!`/`?` followed by whitespace. Closing quotes and brackets between the
/// terminator and the space do not break the run, so `he said "no." then left`
/// still capitalizes `Then`. The known false positive is an abbreviation
/// followed by a space (`e.g. this`); that is the trade every sentence splitter
/// without a lexicon makes, and it only ever changes case, never words.
fn capitalize_sentences(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut at_sentence_start = true;
    let mut after_terminator = false;

    for ch in input.chars() {
        if at_sentence_start && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            at_sentence_start = false;
            after_terminator = false;
            continue;
        }

        out.push(ch);

        match ch {
            '.' | '!' | '?' => after_terminator = true,
            '\n' => {
                at_sentence_start = true;
                after_terminator = false;
            }
            // Trailing quotes/brackets belong to the sentence they close.
            '"' | '\'' | ')' | ']' | '\u{2019}' | '\u{201d}' => {}
            c if c.is_whitespace() => {
                if after_terminator {
                    at_sentence_start = true;
                    after_terminator = false;
                }
            }
            _ => {
                at_sentence_start = false;
                after_terminator = false;
            }
        }
    }
    out
}

/// Append `.` when the text ends mid-word.
///
/// Trailing whitespace is dropped so the period lands against the last word
/// rather than after a stray space. Text already ending in punctuation — or in
/// nothing at all — is left exactly as it is.
fn terminate_sentence(text: &mut String) {
    let end = text.trim_end().len();
    let ends_mid_word = text[..end]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric);
    if ends_mid_word {
        text.truncate(end);
        text.push('.');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Replacements and snippets only; vocabulary is a recognition hint and
    /// plays no part in local formatting.
    fn dict(replacements: &[(&str, &str)], snippets: &[(&str, &str)]) -> DictionaryContext {
        DictionaryContext {
            vocabulary: Vec::new(),
            replacements: replacements
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            snippets: snippets
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// Dictionary stages only, so a test can isolate them from the tidy-up.
    const RAW: PostProcessOptions = PostProcessOptions {
        enabled: true,
        capitalize_sentences: false,
        trailing_punctuation: false,
    };

    const TIDY_ONLY: PostProcessOptions = PostProcessOptions {
        enabled: true,
        capitalize_sentences: true,
        trailing_punctuation: false,
    };

    const TERMINATE_ONLY: PostProcessOptions = PostProcessOptions {
        enabled: true,
        capitalize_sentences: false,
        trailing_punctuation: true,
    };

    #[test]
    fn replacements_run_before_snippets_so_expansions_are_not_rewritten() {
        // "sig" expands to a body containing "kubernetes". If snippets ran
        // first, the replacement rule would then rewrite that body too.
        let d = dict(&[("kubernetes", "K8s")], &[("sig", "kubernetes team")]);
        assert_eq!(
            format_locally("sig and kubernetes", &d, &RAW),
            "kubernetes team and K8s"
        );
    }

    #[test]
    fn dictionary_replacements_are_applied_to_raw_asr() {
        let d = dict(&[("cube er netties", "Kubernetes")], &[]);
        assert_eq!(
            format_locally("deploy to cube er netties now", &d, &RAW),
            "deploy to Kubernetes now"
        );
    }

    #[test]
    fn snippets_are_expanded() {
        let d = dict(&[], &[("my address", "1 Main St")]);
        assert_eq!(
            format_locally("send it to my address", &d, &RAW),
            "send it to 1 Main St"
        );
    }

    #[test]
    fn an_empty_dictionary_leaves_the_transcript_untouched() {
        let d = DictionaryContext::default();
        assert_eq!(format_locally("hello there", &d, &RAW), "hello there");
    }

    #[test]
    fn sentence_casing_capitalizes_the_opening_and_each_following_sentence() {
        let d = DictionaryContext::default();
        assert_eq!(
            format_locally("hello there. how are you? fine!", &d, &TIDY_ONLY),
            "Hello there. How are you? Fine!"
        );
    }

    #[test]
    fn sentence_casing_starts_a_new_sentence_after_a_newline() {
        let d = DictionaryContext::default();
        assert_eq!(
            format_locally("first line\nsecond line", &d, &TIDY_ONLY),
            "First line\nSecond line"
        );
    }

    #[test]
    fn sentence_casing_survives_a_quote_between_the_period_and_the_space() {
        let d = DictionaryContext::default();
        assert_eq!(
            format_locally("he said \"no.\" then left", &d, &TIDY_ONLY),
            "He said \"no.\" Then left"
        );
    }

    #[test]
    fn sentence_casing_does_not_split_a_decimal_number() {
        let d = DictionaryContext::default();
        assert_eq!(
            format_locally("it costs 3.5 dollars", &d, &TIDY_ONLY),
            "It costs 3.5 dollars"
        );
    }

    #[test]
    fn sentence_casing_leaves_already_uppercase_letters_alone() {
        let d = DictionaryContext::default();
        assert_eq!(format_locally("OK then", &d, &TIDY_ONLY), "OK then");
    }

    #[test]
    fn trailing_punctuation_is_added_only_when_the_text_ends_mid_word() {
        let d = DictionaryContext::default();
        assert_eq!(format_locally("all done", &d, &TERMINATE_ONLY), "all done.");
        assert_eq!(
            format_locally("all done!", &d, &TERMINATE_ONLY),
            "all done!"
        );
        assert_eq!(
            format_locally("all done...", &d, &TERMINATE_ONLY),
            "all done..."
        );
        assert_eq!(format_locally("", &d, &TERMINATE_ONLY), "");
    }

    #[test]
    fn trailing_punctuation_replaces_trailing_whitespace_rather_than_following_it() {
        let d = DictionaryContext::default();
        assert_eq!(
            format_locally("all done  ", &d, &TERMINATE_ONLY),
            "all done."
        );
    }

    #[test]
    fn format_if_enabled_yields_nothing_when_post_processing_is_off() {
        let d = dict(&[("cube", "Kube")], &[]);
        assert_eq!(
            format_if_enabled("cube", &d, &PostProcessOptions::DISABLED),
            None
        );
    }

    #[test]
    fn format_if_enabled_yields_the_formatted_text_when_post_processing_is_on() {
        let d = dict(&[("cube", "Kube")], &[]);
        assert_eq!(
            format_if_enabled("cube", &d, &RAW),
            Some("Kube".to_string())
        );
    }

    #[test]
    fn the_whole_pipeline_composes_in_one_pass() {
        let d = dict(
            &[("cube er netties", "Kubernetes")],
            &[("sign off", "Best, Ada")],
        );
        let opts = PostProcessOptions::default();
        assert_eq!(
            format_locally("deploy to cube er netties then sign off", &d, &opts),
            "Deploy to Kubernetes then Best, Ada."
        );
    }

    #[test]
    fn a_replacement_target_is_not_rescanned_by_another_replacement() {
        let mut replacements = BTreeMap::new();
        replacements.insert("a".to_string(), "b".to_string());
        replacements.insert("b".to_string(), "c".to_string());
        let d = DictionaryContext {
            replacements,
            ..Default::default()
        };
        assert_eq!(format_locally("a", &d, &RAW), "b");
    }
}
