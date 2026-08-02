//! Text transforms applied around transcription.
//!
//! Two distinct jobs live here:
//!
//! - **Auto-learn**: mining the difference between raw ASR and the formatted
//!   result for proper nouns worth adding to the user's dictionary.
//! - **Local post-processing**: applying dictionary replacements and snippets
//!   client-side. Wispr does this server-side, but Deepgram returns raw ASR,
//!   so without this a Deepgram user loses their entire dictionary. See
//!   `PORT_PLAN.md` §3.2.

use std::collections::BTreeMap;

/// Mine `formatted` for words the recognizer did not produce, which are
/// therefore corrections worth learning.
///
/// Only capitalized words longer than two characters qualify — the heuristic
/// targets proper nouns (names, products, jargon) and deliberately ignores the
/// far more common case of punctuation and casing fixes.
pub fn auto_learn_candidates(asr_text: &str, formatted_text: &str) -> Vec<String> {
    let asr: std::collections::HashSet<String> =
        asr_text.split_whitespace().map(str::to_lowercase).collect();

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for word in formatted_text.split_whitespace() {
        if asr.contains(&word.to_lowercase()) {
            continue;
        }
        let cleaned = word.trim_matches(|c: char| c.is_ascii_punctuation() || c == '\u{2014}');
        if cleaned.chars().count() <= 2 {
            continue;
        }
        if !cleaned.chars().next().is_some_and(char::is_uppercase) {
            continue;
        }
        if seen.insert(cleaned.to_lowercase()) {
            out.push(cleaned.to_string());
        }
    }
    out
}

/// How the casing of a matched span should be carried onto its replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchCase {
    /// `HELLO` -> replacement uppercased.
    Upper,
    /// `Hello` -> replacement capitalized.
    Title,
    /// Anything else -> replacement used verbatim.
    Verbatim,
}

fn detect_case(matched: &str) -> MatchCase {
    let letters: Vec<char> = matched.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        MatchCase::Upper
    } else if letters.first().is_some_and(|c| c.is_uppercase()) {
        MatchCase::Title
    } else {
        MatchCase::Verbatim
    }
}

fn apply_case(replacement: &str, case: MatchCase) -> String {
    match case {
        MatchCase::Upper => replacement.to_uppercase(),
        MatchCase::Title => {
            let mut chars = replacement.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        MatchCase::Verbatim => replacement.to_string(),
    }
}

/// Replace dictionary phrases in `input`, matching case-insensitively on whole
/// words and carrying the original casing onto the replacement.
///
/// Longer phrases win: with both `"cube"` and `"cube root"` defined, the input
/// `"cube root"` matches the longer entry. Replacements are never re-scanned,
/// so a rule mapping `a -> b` and another mapping `b -> c` cannot cascade.
pub fn apply_replacements(input: &str, replacements: &BTreeMap<String, String>) -> String {
    if replacements.is_empty() || input.is_empty() {
        return input.to_string();
    }

    // Longest first so multi-word phrases beat their own prefixes.
    let mut rules: Vec<(&str, &str)> = replacements
        .iter()
        .filter(|(from, _)| !from.trim().is_empty())
        .map(|(f, t)| (f.as_str(), t.as_str()))
        .collect();
    rules.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));

    let lower_input = input.to_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    'outer: while i < input.len() {
        if !input.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if is_word_start(input, i) {
            for (from, to) in &rules {
                let end = i + from.len();
                if end > input.len() || !input.is_char_boundary(end) {
                    continue;
                }
                if lower_input.get(i..end).map(str::to_string) != Some(from.to_lowercase()) {
                    continue;
                }
                if !is_word_end(input, end) {
                    continue;
                }
                let matched = &input[i..end];
                out.push_str(&apply_case(to, detect_case(matched)));
                i = end;
                continue 'outer;
            }
        }
        let ch = input[i..].chars().next().expect("valid boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Expand dictionary snippets. Identical matching rules to replacements, but
/// kept separate because snippets are a distinct user-facing concept and are
/// applied after replacements.
pub fn expand_snippets(input: &str, snippets: &BTreeMap<String, String>) -> String {
    apply_replacements(input, snippets)
}

fn is_word_start(s: &str, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    s[..i].chars().next_back().is_none_or(|c| !is_word_char(c))
}

fn is_word_end(s: &str, i: usize) -> bool {
    s[i..].chars().next().is_none_or(|c| !is_word_char(c))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Validate a phrase for use as a Deepgram `keyterm`.
///
/// Malformed keyterms are the dangerous case: Deepgram returns HTTP 200 and
/// silently boosts nothing, so a bad entry costs accuracy with no error. `,`,
/// `;` and a trailing `:<weight>` are all accepted by the API and all wrong.
pub fn is_valid_keyterm(phrase: &str) -> bool {
    let p = phrase.trim();
    if p.is_empty() || p.chars().count() > 100 {
        return false;
    }
    if p.contains(',') || p.contains(';') {
        return false;
    }
    if let Some((_, tail)) = p.rsplit_once(':') {
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return false;
        }
    }
    true
}

/// Select the keyterms to send, respecting Deepgram's 500-token budget.
///
/// `phrases` must arrive in priority order (most-used first); this truncates
/// rather than reorders, so the user's most valuable terms survive.
pub fn select_keyterms(phrases: &[String], max_tokens: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut tokens = 0usize;
    for p in phrases {
        if !is_valid_keyterm(p) {
            continue;
        }
        let cost = p.split_whitespace().count().max(1);
        if tokens + cost > max_tokens {
            continue;
        }
        tokens += cost;
        out.push(p.trim().to_string());
    }
    out
}

/// Words in a transcript, matching how the backend counts them.
pub fn word_count(text: &str) -> usize {
    text.split(' ').filter(|s| !s.is_empty()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    // -- auto-learn --------------------------------------------------------

    #[test]
    fn learns_capitalized_words_the_recognizer_missed() {
        let got = auto_learn_candidates(
            "i met with sara about cuba netties",
            "I met with Sarah about Kubernetes",
        );
        assert_eq!(got, vec!["Sarah", "Kubernetes"]);
    }

    #[test]
    fn ignores_words_already_present_in_the_asr_output() {
        // "Sarah" is only a casing fix, not a correction.
        assert!(auto_learn_candidates("i met sarah", "I met Sarah").is_empty());
    }

    #[test]
    fn ignores_lowercase_and_short_words() {
        let got = auto_learn_candidates("aaa", "the of Ok Xyz");
        assert_eq!(got, vec!["Xyz"], "lowercase and <=2 chars are skipped");
    }

    #[test]
    fn strips_surrounding_punctuation_before_learning() {
        assert_eq!(
            auto_learn_candidates("hello", "\"Kubernetes,\""),
            vec!["Kubernetes"]
        );
    }

    #[test]
    fn does_not_learn_the_same_word_twice() {
        assert_eq!(
            auto_learn_candidates("x", "Kubernetes and Kubernetes"),
            vec!["Kubernetes"]
        );
    }

    #[test]
    fn empty_inputs_produce_no_candidates() {
        assert!(auto_learn_candidates("", "").is_empty());
        assert!(auto_learn_candidates("anything", "").is_empty());
    }

    // -- replacements ------------------------------------------------------

    #[test]
    fn replaces_on_whole_word_boundaries_only() {
        let r = map(&[("k8s", "Kubernetes")]);
        assert_eq!(apply_replacements("run k8s now", &r), "run Kubernetes now");
        assert_eq!(
            apply_replacements("k8scluster", &r),
            "k8scluster",
            "a substring inside a longer word must not match"
        );
    }

    #[test]
    fn matches_case_insensitively_and_carries_casing_over() {
        let r = map(&[("k8s", "kubernetes")]);
        assert_eq!(apply_replacements("k8s", &r), "kubernetes");
        assert_eq!(apply_replacements("K8S", &r), "KUBERNETES");
        assert_eq!(apply_replacements("K8s", &r), "Kubernetes");
    }

    #[test]
    fn longer_phrases_win_over_their_own_prefixes() {
        let r = map(&[("cube", "Q"), ("cube root", "\u{221B}")]);
        assert_eq!(apply_replacements("cube root", &r), "\u{221B}");
        assert_eq!(apply_replacements("cube", &r), "Q");
    }

    #[test]
    fn replacements_do_not_cascade() {
        let r = map(&[("a", "b"), ("b", "c")]);
        // "a" becomes "b" and is not re-scanned into "c".
        assert_eq!(apply_replacements("a", &r), "b");
    }

    #[test]
    fn punctuation_around_a_match_is_preserved() {
        let r = map(&[("k8s", "Kubernetes")]);
        assert_eq!(
            apply_replacements("(k8s), k8s.", &r),
            "(Kubernetes), Kubernetes."
        );
    }

    #[test]
    fn multi_word_phrases_match_across_a_space() {
        let r = map(&[("machine learning", "ML")]);
        assert_eq!(
            apply_replacements("I like machine learning a lot", &r),
            "I like ML a lot"
        );
    }

    #[test]
    fn an_empty_ruleset_is_the_identity() {
        assert_eq!(
            apply_replacements("unchanged", &BTreeMap::new()),
            "unchanged"
        );
        assert_eq!(apply_replacements("", &map(&[("a", "b")])), "");
    }

    #[test]
    fn blank_rules_are_ignored_rather_than_matching_everywhere() {
        let r = map(&[("", "BOOM"), ("  ", "BOOM")]);
        assert_eq!(apply_replacements("safe text", &r), "safe text");
    }

    #[test]
    fn non_ascii_text_is_not_corrupted() {
        let r = map(&[("cafe", "caf\u{e9}")]);
        assert_eq!(
            apply_replacements("a cafe \u{4f60}\u{597d}", &r),
            "a caf\u{e9} \u{4f60}\u{597d}"
        );
        // A rule must not split a multi-byte character.
        assert_eq!(
            apply_replacements("\u{4f60}\u{597d}", &map(&[("x", "y")])),
            "\u{4f60}\u{597d}"
        );
    }

    #[test]
    fn snippets_expand_like_replacements() {
        let s = map(&[("addr", "1 Main St")]);
        assert_eq!(expand_snippets("my addr here", &s), "my 1 Main St here");
    }

    // -- keyterms ----------------------------------------------------------

    #[test]
    fn rejects_keyterm_forms_deepgram_silently_ignores() {
        assert!(
            !is_valid_keyterm("term:0.15"),
            "weight syntax is silently ignored"
        );
        assert!(!is_valid_keyterm("a,b"));
        assert!(!is_valid_keyterm("a;b"));
        assert!(!is_valid_keyterm(""));
        assert!(!is_valid_keyterm("   "));
    }

    #[test]
    fn accepts_ordinary_terms_including_multi_word_and_colons_in_names() {
        assert!(is_valid_keyterm("Kubernetes"));
        assert!(is_valid_keyterm("customer service"));
        assert!(is_valid_keyterm("Dr. Smith"));
        assert!(
            is_valid_keyterm("Re: invoices"),
            "a colon not followed by digits is fine"
        );
    }

    #[test]
    fn keyterm_selection_respects_the_token_budget() {
        let phrases: Vec<String> = (0..600).map(|i| format!("term{i}")).collect();
        let picked = select_keyterms(&phrases, 500);
        assert_eq!(picked.len(), 500);
        assert_eq!(picked[0], "term0", "highest-priority terms are kept");
    }

    #[test]
    fn keyterm_selection_counts_multi_word_phrases_correctly() {
        let phrases = vec!["one two three".to_string(), "four".to_string()];
        assert_eq!(select_keyterms(&phrases, 3), vec!["one two three"]);
        assert_eq!(select_keyterms(&phrases, 4), vec!["one two three", "four"]);
    }

    #[test]
    fn keyterm_selection_drops_invalid_entries_without_dropping_the_rest() {
        let phrases = vec!["bad,term".to_string(), "good".to_string()];
        assert_eq!(select_keyterms(&phrases, 500), vec!["good"]);
    }

    #[test]
    fn word_count_matches_the_backend_definition() {
        assert_eq!(word_count("one two three"), 3);
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("  spaced   out  "), 2);
    }
}
