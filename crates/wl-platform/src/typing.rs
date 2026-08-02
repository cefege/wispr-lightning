//! Natural Mode control flow, shared by both platform injectors.
//!
//! Only the *decisions* live here — when to stop, and why. Synthesizing the
//! keystrokes is entirely platform-specific and stays in each injector. The
//! split is what makes this testable: the interesting bugs are an off-by-one
//! in the focus-check cadence, or a cancellation sampled after the keystroke
//! instead of before it, and neither needs a real keyboard to catch.

use crate::Result;

/// Characters typed between checks that the frontmost application is still the
/// one the pass started against.
///
/// Natural Mode types into whatever has focus *right now*, so a user who
/// switches window mid-pass gets the rest of their transcript sprayed into the
/// new one. Checking every character would mean an extra system call per
/// keystroke; eight is under two seconds even at the slowest shipped speed,
/// which keeps the spill down to a few letters.
pub(crate) const FOCUS_CHECK_INTERVAL: usize = 8;

/// Whether the frontmost-application check is due before typing the next
/// character, given how many have already gone out.
///
/// Zero is excluded: the pass has just sampled the frontmost application to
/// get its baseline, so checking again before the first character could only
/// ever compare it with itself.
pub(crate) fn focus_check_due(typed: usize) -> bool {
    typed > 0 && typed % FOCUS_CHECK_INTERVAL == 0
}

/// Why a Natural Mode pass stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypingStop {
    /// Every character went out.
    Completed,
    /// The Escape watcher raised the cancel flag.
    Cancelled,
    /// The user moved to another application mid-pass.
    FocusMoved { from: Option<i32>, to: Option<i32> },
}

/// Drive one Natural Mode pass, returning how many characters went out and why
/// it stopped.
///
/// Stopping early is not an error. The characters that went out went out, the
/// user asked for the rest not to, and the caller retires the transcript the
/// same way it would after a complete pass.
///
/// `post` synthesizes one character and `pause` waits the inter-key gap; both
/// are parameters so the flow can be exercised without a keyboard. `pause` is
/// not called after the final character — nothing is waiting on it.
pub(crate) fn drive_typing(
    text: &str,
    cancelled: &dyn Fn() -> bool,
    frontmost: &dyn Fn() -> Option<i32>,
    post: &mut dyn FnMut(char) -> Result<()>,
    pause: &mut dyn FnMut(),
) -> Result<(usize, TypingStop)> {
    let initial = frontmost();
    let mut typed = 0usize;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        // Both checks precede the post, so the character that trips them is
        // the first one *not* typed rather than the last one that was.
        if cancelled() {
            return Ok((typed, TypingStop::Cancelled));
        }
        if focus_check_due(typed) {
            let now = frontmost();
            if now != initial {
                return Ok((
                    typed,
                    TypingStop::FocusMoved {
                        from: initial,
                        to: now,
                    },
                ));
            }
        }
        post(ch)?;
        typed += 1;
        if chars.peek().is_some() {
            pause();
        }
    }
    Ok((typed, TypingStop::Completed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Run a pass over `text`, cancelling once `cancel_after` characters have
    /// gone out, against a frontmost pid that switches from 100 to `moves_to`
    /// once `moves_after` characters have gone out.
    fn run(
        text: &str,
        cancel_after: Option<usize>,
        moves_after: Option<usize>,
        moves_to: Option<i32>,
    ) -> (String, usize, TypingStop) {
        let typed = Cell::new(0usize);
        let out = Cell::new(String::new());
        let pauses = Cell::new(0usize);

        let (count, stop) = drive_typing(
            text,
            &|| cancel_after.is_some_and(|n| typed.get() >= n),
            &|| match moves_after {
                Some(n) if typed.get() >= n => moves_to,
                _ => Some(100),
            },
            &mut |ch| {
                let mut s = out.take();
                s.push(ch);
                out.set(s);
                typed.set(typed.get() + 1);
                Ok(())
            },
            &mut || pauses.set(pauses.get() + 1),
        )
        .expect("the fake post never fails");

        // A completed pass pauses between characters and not after the last
        // one. A pass that stopped early paused after its last character too:
        // the abort was noticed on the *next* iteration, by which point the
        // gap had already been waited.
        let expected = match stop {
            TypingStop::Completed => count.saturating_sub(1),
            _ => count,
        };
        assert_eq!(
            pauses.get(),
            expected,
            "one pause between each pair of characters, none after the last"
        );
        (out.take(), count, stop)
    }

    #[test]
    fn an_uninterrupted_pass_types_every_character() {
        let (out, count, stop) = run("hello world", None, None, None);
        assert_eq!(out, "hello world");
        assert_eq!(count, 11);
        assert_eq!(stop, TypingStop::Completed);
    }

    /// The whole point of Escape: the characters after the cancellation must
    /// never reach the target, not merely be reported as skipped.
    #[test]
    fn cancellation_short_circuits_the_character_loop() {
        let (out, count, stop) = run("abcdefghij", Some(4), None, None);
        assert_eq!(out, "abcd", "characters kept going out after the cancel");
        assert_eq!(count, 4);
        assert_eq!(stop, TypingStop::Cancelled);
    }

    /// A cancel that is already raised when the pass starts must produce no
    /// keystrokes at all — the check has to precede the first post.
    #[test]
    fn a_cancel_raised_before_the_first_character_types_nothing() {
        let (out, count, stop) = run("abc", Some(0), None, None);
        assert!(out.is_empty(), "typed {out:?} after an up-front cancel");
        assert_eq!(count, 0);
        assert_eq!(stop, TypingStop::Cancelled);
    }

    /// Focus is sampled every eighth character, so a switch that happens at
    /// character 3 is not noticed until the check at 8 — and the pass must
    /// stop exactly there, not at 3 and not at 9.
    #[test]
    fn a_focus_change_is_caught_at_the_next_eight_character_boundary() {
        let (out, count, stop) = run("abcdefghijklmnop", None, Some(3), Some(200));
        assert_eq!(out, "abcdefgh");
        assert_eq!(count, FOCUS_CHECK_INTERVAL);
        assert_eq!(
            stop,
            TypingStop::FocusMoved {
                from: Some(100),
                to: Some(200)
            }
        );
    }

    /// Sixteen characters means two checks, so a switch after the first one
    /// must still be caught by the second.
    #[test]
    fn the_focus_check_repeats_every_eight_characters() {
        let (_, count, stop) = run("abcdefghijklmnopqrstuvwx", None, Some(9), Some(200));
        assert_eq!(count, 2 * FOCUS_CHECK_INTERVAL);
        assert_eq!(
            stop,
            TypingStop::FocusMoved {
                from: Some(100),
                to: Some(200)
            }
        );
    }

    /// A text shorter than the interval is never focus-checked at all, which
    /// is the cheap path most dictated fragments take.
    #[test]
    fn a_short_pass_never_reaches_a_focus_check() {
        let (out, count, stop) = run("abcdefg", None, Some(0), Some(200));
        assert_eq!(out, "abcdefg");
        assert_eq!(count, 7);
        assert_eq!(stop, TypingStop::Completed);
    }

    /// AppKit refusing to name the frontmost application is not a focus
    /// change: an unreadable baseline that stays unreadable must not abort a
    /// perfectly good pass.
    #[test]
    fn an_unreadable_frontmost_application_does_not_count_as_a_move() {
        let stop = drive_typing(
            "abcdefghijkl",
            &|| false,
            &|| None,
            &mut |_| Ok(()),
            &mut || {},
        )
        .expect("the fake post never fails");
        assert_eq!(stop, (12, TypingStop::Completed));
    }

    /// Cancellation wins a tie: when both would fire on the same character,
    /// the deliberate user action is the one worth reporting.
    #[test]
    fn cancellation_takes_precedence_over_a_simultaneous_focus_change() {
        let (_, count, stop) = run("abcdefghijkl", Some(8), Some(8), Some(200));
        assert_eq!(count, FOCUS_CHECK_INTERVAL);
        assert_eq!(stop, TypingStop::Cancelled);
    }

    #[test]
    fn the_focus_check_cadence_is_every_eighth_character() {
        assert!(!focus_check_due(0), "checked before typing anything");
        for typed in 1..FOCUS_CHECK_INTERVAL {
            assert!(!focus_check_due(typed), "checked early at {typed}");
        }
        assert!(focus_check_due(FOCUS_CHECK_INTERVAL));
        for typed in FOCUS_CHECK_INTERVAL + 1..2 * FOCUS_CHECK_INTERVAL {
            assert!(!focus_check_due(typed), "checked early at {typed}");
        }
        assert!(focus_check_due(2 * FOCUS_CHECK_INTERVAL));
        assert!(focus_check_due(24));
    }

    #[test]
    fn an_empty_text_completes_without_typing_anything() {
        let (out, count, stop) = run("", None, None, None);
        assert!(out.is_empty());
        assert_eq!(count, 0);
        assert_eq!(stop, TypingStop::Completed);
    }
}
