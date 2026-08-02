//! The chord guard: telling a push-to-talk modifier apart from a shortcut
//! modifier.
//!
//! # The hazard
//!
//! The shipped default binds dictation to a **bare Left Control**, held. A
//! bare modifier is the only binding that can collide with the user's ordinary
//! typing, because a modifier is also the prefix of almost every keyboard
//! shortcut. On macOS that is survivable — ⌘ is the shortcut modifier and
//! Control is comparatively free. On Windows, Control *is* the shortcut
//! modifier, and the collision is `Ctrl+C`:
//!
//! * Control goes down, so the app starts recording.
//! * `C` is typed and Control comes back up well inside
//!   [`wl_core::fsm::LOCK_DEBOUNCE`], so the state machine schedules a stop
//!   half a second after the press. The app records half a second of room
//!   noise, transcribes it, and types the result into the user's document.
//! * A `Ctrl+V` inside the same half second is a second press inside the lock
//!   window, so the machine reads it as a **double tap and locks hands-free
//!   recording**. Copy-paste, the most common key sequence on the platform,
//!   leaves a silent open microphone.
//!
//! # The rule
//!
//! While a **modifier-only** binding is **physically held**, a non-modifier
//! key going down means the user is typing a shortcut. The hold is abandoned:
//! the backend emits [`crate::hotkey::Transition::Aborted`] instead of the
//! release it would otherwise owe, the pipeline discards the audio, and the
//! binding is *disarmed* until the user actually lets the modifier go, so the
//! release of the `Ctrl` in `Ctrl+C` cannot start a fresh dictation.
//!
//! Four limits, each load-bearing:
//!
//! * **Modifier-only bindings only.** `Ctrl+Shift` is the user asking for a
//!   chord; it already cannot be typed by accident and keeps working.
//! * **Held bindings only.** In hands-free mode nothing is latched, so a
//!   `Ctrl+C` there is an ordinary shortcut and the guard never sees a hold to
//!   cancel. (The pipeline gates the abort on the machine actually being in
//!   push-to-talk as well, for the one instant where a locking press is still
//!   physically down.)
//! * **Never suppressed.** Both backends run their listener in observe-only
//!   mode, so the keystroke reaches the focused app untouched. `Ctrl+C` still
//!   copies. Nothing here blocks, consumes or re-posts an event.
//! * **Never tripped by our own input.** The guard runs downstream of the
//!   synthetic-input filter on both platforms, so paste and Natural Mode
//!   keystrokes are invisible to it. That ordering is not incidental.
//!
//! # Deliberate deviation from the Swift original
//!
//! `HotkeyListener.swift` had no such guard: any key was welcome to go down
//! mid-hold and the recording ran on. This module is the one place the port
//! knowingly does something the original did not, and it is a bug fix rather
//! than a redesign. The old behaviour is still reachable by binding a chord
//! or a trigger key instead of a bare modifier.

use handy_keys::Key as HkKey;

/// Whether `key` going down should be read as the user typing a shortcut.
///
/// Everything that is a real keystroke counts, the trigger keys a binding
/// could itself use included: with dictation on bare Left Control, `Ctrl+Space`
/// is an input-method toggle, not a dictation.
///
/// Two families are excluded, and neither exclusion is a judgement call about
/// how shortcut-like a key feels:
///
/// * **Mouse buttons.** `handy-keys` reports modifier-qualified clicks through
///   the same `Key` enum, and Control-clicking mid-sentence is something users
///   do on purpose. A click is not a keystroke.
/// * **Lock keys.** Caps Lock and friends arrive as `FlagsChanged` on macOS
///   and their `is_key_down` carries the *lamp state*, not an edge — a "down"
///   here does not mean the user just pressed anything, and a Caps Lock that
///   was already on would abort every hold.
pub fn interrupts_chord(key: HkKey) -> bool {
    !matches!(
        key,
        HkKey::MouseLeft
            | HkKey::MouseRight
            | HkKey::MouseMiddle
            | HkKey::MouseX1
            | HkKey::MouseX2
            | HkKey::CapsLock
            | HkKey::NumLock
            | HkKey::ScrollLock
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sequence the guard exists for.
    #[test]
    fn a_letter_is_the_user_typing_a_shortcut() {
        assert!(interrupts_chord(HkKey::C));
        assert!(interrupts_chord(HkKey::V));
        assert!(interrupts_chord(HkKey::Z));
        assert!(interrupts_chord(HkKey::Num4));
    }

    /// A key that could legally be a trigger is still a shortcut when it lands
    /// under a bare modifier the user is holding for something else.
    #[test]
    fn a_trigger_vocabulary_key_still_interrupts() {
        for key in [HkKey::Space, HkKey::Return, HkKey::Tab, HkKey::Escape] {
            assert!(interrupts_chord(key), "{key:?}");
        }
        assert!(interrupts_chord(HkKey::F5));
    }

    /// Control-clicking while dictating is deliberate, not a shortcut. macOS
    /// delivers exactly these through the same channel whenever a modifier is
    /// held, which is precisely when the guard is armed.
    #[test]
    fn a_mouse_button_never_interrupts() {
        for key in [
            HkKey::MouseLeft,
            HkKey::MouseRight,
            HkKey::MouseMiddle,
            HkKey::MouseX1,
            HkKey::MouseX2,
        ] {
            assert!(!interrupts_chord(key), "{key:?}");
        }
    }

    /// `is_key_down` for a lock key is its lamp, so treating it as an edge
    /// would abort every hold taken with Caps Lock already on.
    #[test]
    fn a_lock_key_never_interrupts() {
        for key in [HkKey::CapsLock, HkKey::NumLock, HkKey::ScrollLock] {
            assert!(!interrupts_chord(key), "{key:?}");
        }
    }
}
