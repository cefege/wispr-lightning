//! Pure hotkey matching: `handy-keys` key events in, [`HotkeyEvent`]s out.
//!
//! This is the part of the Windows hotkey backend with actual behaviour in it,
//! so it is kept free of Win32 and of `handy-keys`' threading in order to stay
//! testable on any host.
//!
//! One rule from `HotkeyListener.swift` is load-bearing and reproduced here:
//! * **Press/release asymmetry.** A press is only accepted when the backend is
//!   not paused; a release is delivered unconditionally, because a recording
//!   that started before the pause must still be able to stop.
//!
//! Unlike macOS — where left and right Control share one `CGEventFlags` bit —
//! Windows reports the two separately, so a binding on Left Control is not
//! ended by releasing Right Control. That is a deliberate divergence in the
//! direction of correctness (PORT_PLAN §10.3).
//!
//! One rule here has no Swift ancestor at all: [`Latch::guard`], the chord
//! guard. Control is the shortcut modifier on Windows and the shipped default
//! binds dictation to a bare Control, so without it `Ctrl+C` starts a
//! recording and `Ctrl+C` then `Ctrl+V` locks a hands-free one. See
//! [`crate::chord`] for the full argument.

use handy_keys::{Key as HkKey, KeyEvent as HkKeyEvent, Modifiers as HkModifiers};
use wl_core::settings::hotkey::{Hotkey, Modifiers, TriggerKey};

use crate::chord::interrupts_chord;
use crate::hotkey::{Binding, HotkeyEvent, Transition};

/// `handy-keys` modifier flag paired with its portable equivalent.
///
/// Only side-specific flags appear: the compound "either side" aliases would
/// make `contains` ambiguous and merge independent user bindings.
const MODIFIER_PAIRS: &[(HkModifiers, Modifiers)] = &[
    (HkModifiers::CTRL_LEFT, Modifiers::CTRL_LEFT),
    (HkModifiers::CTRL_RIGHT, Modifiers::CTRL_RIGHT),
    (HkModifiers::OPT_LEFT, Modifiers::ALT_LEFT),
    (HkModifiers::OPT_RIGHT, Modifiers::ALT_RIGHT),
    (HkModifiers::CMD_LEFT, Modifiers::META_LEFT),
    (HkModifiers::CMD_RIGHT, Modifiers::META_RIGHT),
    (HkModifiers::SHIFT_LEFT, Modifiers::SHIFT_LEFT),
    (HkModifiers::SHIFT_RIGHT, Modifiers::SHIFT_RIGHT),
    (HkModifiers::FN, Modifiers::FN),
];

/// One keyboard transition, reduced to the portable vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Edge {
    /// Every modifier held *after* this transition.
    pub(crate) modifiers: Modifiers,
    /// The non-modifier key that changed, or `None` for a modifier-only event.
    pub(crate) key: Option<TriggerKey>,
    pub(crate) down: bool,
}

pub(crate) fn to_core_modifiers(modifiers: HkModifiers) -> Modifiers {
    MODIFIER_PAIRS
        .iter()
        .filter(|(hk, _)| modifiers.contains(*hk))
        .fold(Modifiers::NONE, |acc, (_, core)| acc | *core)
}

/// The portable trigger for a `handy-keys` key, if we can express it.
///
/// `TriggerKey` covers exactly the non-modifier keys the settings model allows
/// as a push-to-talk trigger; everything else is ordinary typing.
pub(crate) fn to_trigger_key(key: HkKey) -> Option<TriggerKey> {
    Some(match key {
        HkKey::Return | HkKey::KeypadEnter => TriggerKey::Return,
        HkKey::Space => TriggerKey::Space,
        HkKey::Escape => TriggerKey::Escape,
        HkKey::Tab => TriggerKey::Tab,
        HkKey::F1 => TriggerKey::F(1),
        HkKey::F2 => TriggerKey::F(2),
        HkKey::F3 => TriggerKey::F(3),
        HkKey::F4 => TriggerKey::F(4),
        HkKey::F5 => TriggerKey::F(5),
        HkKey::F6 => TriggerKey::F(6),
        HkKey::F7 => TriggerKey::F(7),
        HkKey::F8 => TriggerKey::F(8),
        HkKey::F9 => TriggerKey::F(9),
        HkKey::F10 => TriggerKey::F(10),
        HkKey::F11 => TriggerKey::F(11),
        HkKey::F12 => TriggerKey::F(12),
        HkKey::F13 => TriggerKey::F(13),
        HkKey::F14 => TriggerKey::F(14),
        HkKey::F15 => TriggerKey::F(15),
        HkKey::F16 => TriggerKey::F(16),
        HkKey::F17 => TriggerKey::F(17),
        HkKey::F18 => TriggerKey::F(18),
        HkKey::F19 => TriggerKey::F(19),
        HkKey::F20 => TriggerKey::F(20),
        HkKey::F21 => TriggerKey::F(21),
        HkKey::F22 => TriggerKey::F(22),
        HkKey::F23 => TriggerKey::F(23),
        HkKey::F24 => TriggerKey::F(24),
        _ => return None,
    })
}

/// Reduce a raw listener event to an [`Edge`].
///
/// Returns `None` for keys outside the trigger vocabulary: they can neither
/// start nor end a binding, and the modifier state they carry is already
/// tracked by the modifier events around them.
pub(crate) fn edge_from(event: &HkKeyEvent) -> Option<Edge> {
    let key = match event.key {
        None => None,
        Some(key) => Some(to_trigger_key(key)?),
    };
    Some(Edge {
        modifiers: to_core_modifiers(event.modifiers),
        key,
        down: event.is_key_down,
    })
}

/// A binding with no modifiers and no key would match every event; the
/// settings UI cannot produce one, but a hand-edited file can.
fn is_bindable(hotkey: &Hotkey) -> bool {
    !hotkey.modifiers.is_empty() || hotkey.key.is_some()
}

/// Whether this binding is nothing but modifiers, which is what makes it
/// indistinguishable from the prefix of a keyboard shortcut and therefore the
/// only kind [`Latch::guard`] applies to.
///
/// The number of modifiers is deliberately **not** part of the test, and this
/// predicate is not too broad. `Ctrl+Shift+S` and `Ctrl+Shift+Esc` are every
/// bit as real as `Ctrl+C`, so a user who rebinds to a two-modifier
/// combination has exactly the same hazard; a modifier-count check here would
/// protect only the shipped default and leave everyone else exposed. Nor does
/// it cost a `Ctrl+Shift` binding anything: it still starts, holds and stops
/// normally, and the guard speaks up only once a *third*, non-modifier key
/// goes down — at which point the user was typing a shortcut, not dictating.
///
/// Bindings that carry a trigger key (`Ctrl+Space`, a bare `F13`) are the
/// exclusion: they cannot be typed by accident, so the guard leaves them be.
fn is_modifier_only(hotkey: &Hotkey) -> bool {
    hotkey.key.is_none() && !hotkey.modifiers.is_empty()
}

fn starts(hotkey: &Hotkey, edge: &Edge) -> bool {
    edge.down && edge.key == hotkey.key && edge.modifiers.contains(hotkey.modifiers)
}

fn ends(hotkey: &Hotkey, edge: &Edge) -> bool {
    if edge.down {
        return false;
    }
    match hotkey.key {
        // A trigger key ends when that key comes up, whatever the modifiers
        // are doing — releasing Shift first must not strand the binding.
        Some(_) => edge.key == hotkey.key,
        // A bare modifier hold ends when the required modifiers are no longer
        // all held.
        None => !edge.modifiers.contains(hotkey.modifiers),
    }
}

/// Which binding, if any, is currently held down.
#[derive(Debug, Default)]
pub(crate) struct Latch {
    dictate: Option<Hotkey>,
    /// Bindings [`Latch::guard`] cancelled, kept by value rather than by slot
    /// so a rebind mid-hold cannot hand one back.
    ///
    /// A binding in here is physically held but must not start anything: the
    /// user is mid-shortcut. Entries leave in [`Latch::apply`], on the first
    /// transition that shows the modifiers are no longer all down.
    disarmed: Vec<Hotkey>,
}

impl Latch {
    /// Forget any held key, e.g. after a rebind or a pause toggle.
    ///
    /// Deliberately silent: the caller wants the physically-held key to stop
    /// counting, not a release event for a press the app has already handled.
    ///
    /// Just as deliberately, this does **not** clear `disarmed`. The pipeline
    /// resets from its abort path, which is the path a chord abort itself
    /// takes; forgetting here would let the still-held Control of a `Ctrl+C`
    /// start a fresh dictation the moment the user let go, closing the hole
    /// and reopening it in one breath. The two say opposite things: this one
    /// says "nothing is held", `disarmed` says "something is held and must
    /// not count".
    pub(crate) fn reset(&mut self) {
        self.dictate = None;
    }

    /// The chord guard: a keystroke arrived, so cancel every held
    /// modifier-only binding.
    ///
    /// A bare modifier under a keystroke was a shortcut prefix, not a
    /// push-to-talk trigger — see [`crate::chord`] for why this is the app's
    /// single most consequential default. The binding is dropped from the
    /// latch *and* remembered, so the modifier release that follows neither
    /// reports a stop the app never started nor starts a new dictation.
    ///
    /// Observes only: the caller must not consume the event, or `Ctrl+C` would
    /// stop copying. Idempotent under key auto-repeat, because the second call
    /// finds nothing latched.
    pub(crate) fn guard(&mut self, event: &HkKeyEvent) -> Vec<HotkeyEvent> {
        if !event.is_key_down || !event.key.is_some_and(interrupts_chord) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.dictate.as_ref().is_some_and(is_modifier_only) {
            let hotkey = self.dictate.take().expect("checked above");
            self.disarmed.push(hotkey);
            out.push(HotkeyEvent {
                binding: Binding::Dictate,
                transition: Transition::Aborted,
            });
        }
        out
    }

    /// Feed one transition and collect whatever it means.
    ///
    /// At most one event per edge in practice; the return type is a `Vec`
    /// (which does not allocate when empty) so that a reconciliation event
    /// clearing several modifiers at once cannot strand a latched binding.
    pub(crate) fn apply(
        &mut self,
        edge: &Edge,
        dictate: &[Hotkey],
        suppress_presses: bool,
    ) -> Vec<HotkeyEvent> {
        let mut out = Vec::new();
        self.disarmed
            .retain(|hotkey| edge.modifiers.contains(hotkey.modifiers));

        if self
            .dictate
            .as_ref()
            .is_some_and(|hotkey| ends(hotkey, edge))
        {
            self.dictate = None;
            out.push(HotkeyEvent {
                binding: Binding::Dictate,
                transition: Transition::Released,
            });
        }

        if suppress_presses || !edge.down {
            return out;
        }

        if self.dictate.is_none() {
            if let Some(hotkey) = dictate
                .iter()
                .find(|h| is_bindable(h) && !self.disarmed.contains(h) && starts(h, edge))
            {
                self.dictate = Some(*hotkey);
                out.push(HotkeyEvent {
                    binding: Binding::Dictate,
                    transition: Transition::Pressed,
                });
                return out;
            }
        }

        out
    }
}

/// Records the chord a user presses while the settings UI is asking for one.
///
/// The resolution rule is what makes a bare-modifier binding recordable at
/// all: latching on the first modifier *down* would make "Left Ctrl + Shift"
/// unreachable, since Ctrl alone would already have won. So modifiers
/// accumulate while they are held and the chord resolves either on the first
/// non-modifier key or when the last modifier comes back up.
#[derive(Debug, Default)]
pub(crate) struct Capture {
    /// Largest modifier set seen while keys were held.
    modifiers: Modifiers,
    /// Resolved result, if the user has finished the gesture.
    resolved: Option<Hotkey>,
    /// The user pressed a key `Hotkey` cannot express.
    rejected: bool,
}

impl Capture {
    pub(crate) fn observe(&mut self, edge: &Edge) {
        if self.resolved.is_some() || self.rejected {
            return;
        }
        match (edge.key, edge.down) {
            // A non-modifier key resolves immediately, with whatever modifiers
            // are held alongside it.
            (Some(key), true) => {
                self.modifiers |= edge.modifiers;
                self.resolved = Some(Hotkey {
                    modifiers: self.modifiers,
                    key: Some(key),
                });
            }
            (Some(_), false) => {}
            // Modifiers accumulate on the way down...
            (None, true) => self.modifiers |= edge.modifiers,
            // ...and the gesture ends when the last one is let go.
            (None, false) => {
                if edge.modifiers.is_empty() && !self.modifiers.is_empty() {
                    self.resolved = Some(Hotkey {
                        modifiers: self.modifiers,
                        key: None,
                    });
                }
            }
        }
    }

    /// Note a key press that [`Hotkey`] has no representation for.
    ///
    /// `TriggerKey` covers modifiers plus Return, Space, Escape, Tab and the
    /// function keys, so "Ctrl+Shift+K" is unrecordable. Abandoning the
    /// gesture is the only honest answer: silently storing "Ctrl+Shift" would
    /// bind a hotkey the user never asked for and did not see.
    pub(crate) fn reject(&mut self) {
        if self.resolved.is_none() {
            self.rejected = true;
        }
    }

    /// The recorded chord, or `None` if the user pressed nothing usable.
    ///
    /// `None` means "cancelled": the caller leaves the existing binding alone.
    /// A partially-observed gesture (modifiers still held when the settings
    /// window closed) counts as nothing usable, as does a rejected one.
    pub(crate) fn finish(self) -> Option<Hotkey> {
        self.resolved.filter(|_| !self.rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare(modifiers: Modifiers) -> Hotkey {
        Hotkey {
            modifiers,
            key: None,
        }
    }

    fn edge(modifiers: Modifiers, down: bool) -> Edge {
        Edge {
            modifiers,
            key: None,
            down,
        }
    }

    fn keyed(modifiers: Modifiers, key: TriggerKey, down: bool) -> Edge {
        Edge {
            modifiers,
            key: Some(key),
            down,
        }
    }

    fn press(binding: Binding) -> HotkeyEvent {
        HotkeyEvent {
            binding,
            transition: Transition::Pressed,
        }
    }

    fn release(binding: Binding) -> HotkeyEvent {
        HotkeyEvent {
            binding,
            transition: Transition::Released,
        }
    }

    #[test]
    fn side_specific_modifiers_survive_the_translation_from_handy_keys() {
        assert_eq!(
            to_core_modifiers(HkModifiers::CTRL_LEFT),
            Modifiers::CTRL_LEFT
        );
        assert_eq!(
            to_core_modifiers(HkModifiers::OPT_RIGHT | HkModifiers::FN),
            Modifiers::ALT_RIGHT | Modifiers::FN
        );
        // handy-keys calls it Cmd, we call it Meta, and the sides must not swap.
        assert_eq!(
            to_core_modifiers(HkModifiers::CMD_LEFT),
            Modifiers::META_LEFT
        );
        assert_eq!(to_core_modifiers(HkModifiers::empty()), Modifiers::NONE);
    }

    #[test]
    fn only_keys_the_settings_model_can_express_become_triggers() {
        assert_eq!(to_trigger_key(HkKey::Space), Some(TriggerKey::Space));
        assert_eq!(to_trigger_key(HkKey::F13), Some(TriggerKey::F(13)));
        assert_eq!(to_trigger_key(HkKey::A), None);
        assert_eq!(to_trigger_key(HkKey::MouseLeft), None);
    }

    #[test]
    fn a_bare_modifier_hold_reports_one_press_and_one_release() {
        let mut latch = Latch::default();
        let dictate = [bare(Modifiers::CTRL_LEFT)];

        let down = latch.apply(&edge(Modifiers::CTRL_LEFT, true), &dictate, false);
        assert_eq!(down, vec![press(Binding::Dictate)]);

        let up = latch.apply(&edge(Modifiers::NONE, false), &dictate, false);
        assert_eq!(up, vec![release(Binding::Dictate)]);
    }

    #[test]
    fn auto_repeat_does_not_produce_a_second_press() {
        let mut latch = Latch::default();
        let dictate = [bare(Modifiers::CTRL_LEFT)];
        let held = edge(Modifiers::CTRL_LEFT, true);

        assert_eq!(
            latch.apply(&held, &dictate, false),
            vec![press(Binding::Dictate)]
        );
        assert!(latch.apply(&held, &dictate, false).is_empty());
        assert!(latch.apply(&held, &dictate, false).is_empty());
    }

    #[test]
    fn suppressing_presses_never_suppresses_the_release_of_a_live_hold() {
        let mut latch = Latch::default();
        let dictate = [bare(Modifiers::CTRL_LEFT)];

        // Started before the pause.
        latch.apply(&edge(Modifiers::CTRL_LEFT, true), &dictate, false);
        // Paused mid-hold: the release must still arrive or the app wedges.
        assert_eq!(
            latch.apply(&edge(Modifiers::NONE, false), &dictate, true),
            vec![release(Binding::Dictate)]
        );
        // And a fresh press while paused is ignored.
        assert!(latch
            .apply(&edge(Modifiers::CTRL_LEFT, true), &dictate, true)
            .is_empty());
    }

    #[test]
    fn alternatives_in_one_list_are_independent_triggers_not_a_chord() {
        let mut latch = Latch::default();
        let dictate = [bare(Modifiers::CTRL_LEFT), bare(Modifiers::SHIFT_RIGHT)];

        assert_eq!(
            latch.apply(&edge(Modifiers::SHIFT_RIGHT, true), &dictate, false),
            vec![press(Binding::Dictate)]
        );
        assert_eq!(
            latch.apply(&edge(Modifiers::NONE, false), &dictate, false),
            vec![release(Binding::Dictate)]
        );
    }

    #[test]
    fn a_modifier_plus_key_binding_ends_on_the_keys_release_not_the_modifiers() {
        let mut latch = Latch::default();
        let dictate = [Hotkey {
            modifiers: Modifiers::ALT_LEFT,
            key: Some(TriggerKey::Space),
        }];

        assert!(latch
            .apply(&edge(Modifiers::ALT_LEFT, true), &dictate, false)
            .is_empty());
        assert_eq!(
            latch.apply(
                &keyed(Modifiers::ALT_LEFT, TriggerKey::Space, true),
                &dictate,
                false
            ),
            vec![press(Binding::Dictate)]
        );
        // Alt released first: the hold survives, because the key is still down.
        assert!(latch
            .apply(&edge(Modifiers::NONE, false), &dictate, false)
            .is_empty());
        assert_eq!(
            latch.apply(
                &keyed(Modifiers::NONE, TriggerKey::Space, false),
                &dictate,
                false
            ),
            vec![release(Binding::Dictate)]
        );
    }

    #[test]
    fn a_key_without_its_modifier_does_not_trigger() {
        let mut latch = Latch::default();
        let dictate = [Hotkey {
            modifiers: Modifiers::ALT_LEFT,
            key: Some(TriggerKey::Space),
        }];
        assert!(latch
            .apply(
                &keyed(Modifiers::NONE, TriggerKey::Space, true),
                &dictate,
                false
            )
            .is_empty());
    }

    #[test]
    fn reset_drops_the_hold_without_emitting_a_release() {
        let mut latch = Latch::default();
        let dictate = [bare(Modifiers::CTRL_LEFT)];

        latch.apply(&edge(Modifiers::CTRL_LEFT, true), &dictate, false);
        latch.reset();

        // The physical key coming up must now be a no-op, not a stray release.
        assert!(latch
            .apply(&edge(Modifiers::NONE, false), &dictate, false)
            .is_empty());
        // And the next press starts cleanly.
        assert_eq!(
            latch.apply(&edge(Modifiers::CTRL_LEFT, true), &dictate, false),
            vec![press(Binding::Dictate)]
        );
    }

    #[test]
    fn an_empty_binding_from_a_hand_edited_settings_file_never_matches() {
        let mut latch = Latch::default();
        let broken = [bare(Modifiers::NONE)];
        assert!(latch
            .apply(&edge(Modifiers::CTRL_LEFT, true), &broken, false)
            .is_empty());
    }

    #[test]
    fn ordinary_typing_is_not_an_edge_at_all() {
        let typing = HkKeyEvent {
            modifiers: HkModifiers::empty(),
            key: Some(HkKey::A),
            is_key_down: true,
            changed_modifier: None,
        };
        assert!(edge_from(&typing).is_none());

        let modifier = HkKeyEvent {
            modifiers: HkModifiers::CTRL_LEFT,
            key: None,
            is_key_down: true,
            changed_modifier: Some(HkModifiers::CTRL_LEFT),
        };
        assert_eq!(
            edge_from(&modifier),
            Some(Edge {
                modifiers: Modifiers::CTRL_LEFT,
                key: None,
                down: true
            })
        );
    }

    #[test]
    fn capturing_a_bare_modifier_resolves_when_it_is_released() {
        let mut capture = Capture::default();
        capture.observe(&edge(Modifiers::CTRL_LEFT, true));
        capture.observe(&edge(Modifiers::NONE, false));
        assert_eq!(capture.finish(), Some(bare(Modifiers::CTRL_LEFT)));
    }

    #[test]
    fn capturing_keeps_every_modifier_that_was_held_during_the_gesture() {
        let mut capture = Capture::default();
        capture.observe(&edge(Modifiers::CTRL_LEFT, true));
        capture.observe(&edge(Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT, true));
        capture.observe(&edge(Modifiers::CTRL_LEFT, false));
        capture.observe(&edge(Modifiers::NONE, false));
        assert_eq!(
            capture.finish(),
            Some(bare(Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT))
        );
    }

    #[test]
    fn capturing_a_modifier_plus_key_resolves_on_the_key() {
        let mut capture = Capture::default();
        capture.observe(&edge(Modifiers::ALT_LEFT, true));
        capture.observe(&keyed(Modifiers::ALT_LEFT, TriggerKey::Space, true));
        capture.observe(&edge(Modifiers::NONE, false));
        assert_eq!(
            capture.finish(),
            Some(Hotkey {
                modifiers: Modifiers::ALT_LEFT,
                key: Some(TriggerKey::Space)
            })
        );
    }

    #[test]
    fn an_abandoned_capture_reports_nothing_rather_than_an_empty_binding() {
        // Nothing pressed at all.
        assert!(Capture::default().finish().is_none());

        // Modifier still physically held when the window closed.
        let mut capture = Capture::default();
        capture.observe(&edge(Modifiers::CTRL_LEFT, true));
        assert!(capture.finish().is_none());
    }

    #[test]
    fn a_key_the_settings_model_cannot_express_cancels_the_capture() {
        let mut capture = Capture::default();
        capture.observe(&edge(Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT, true));
        // The user pressed "K", which `TriggerKey` has no variant for. Storing
        // the modifiers alone would bind Ctrl+Shift behind their back.
        capture.reject();
        capture.observe(&edge(Modifiers::NONE, false));
        assert!(capture.finish().is_none());
    }

    // -- The chord guard ---------------------------------------------------

    fn abort(binding: Binding) -> HotkeyEvent {
        HotkeyEvent {
            binding,
            transition: Transition::Aborted,
        }
    }

    /// A latch driven exactly the way the pump drives it: raw event to the
    /// chord guard first, then — only for the transitions the settings model
    /// can express — on to the edge matching.
    ///
    /// Written this way on purpose. The guard's whole point is that it sees
    /// keys `edge_from` throws away, so testing the two halves separately
    /// would test everything except the ordering they depend on.
    struct Keyboard {
        latch: Latch,
        dictate: Vec<Hotkey>,
        paused: bool,
    }

    impl Keyboard {
        fn new(dictate: &[Hotkey]) -> Self {
            Self {
                latch: Latch::default(),
                dictate: dictate.to_vec(),
                paused: false,
            }
        }

        /// A modifier going down or up. `now` is the state *after* the change,
        /// which is what the hook reports.
        fn modifiers(&mut self, now: HkModifiers, down: bool) -> Vec<HotkeyEvent> {
            self.feed(now, None, down)
        }

        /// An ordinary key going down or up under `now`.
        fn key(&mut self, now: HkModifiers, key: HkKey, down: bool) -> Vec<HotkeyEvent> {
            self.feed(now, Some(key), down)
        }

        fn feed(&mut self, now: HkModifiers, key: Option<HkKey>, down: bool) -> Vec<HotkeyEvent> {
            let event = HkKeyEvent {
                modifiers: now,
                key,
                is_key_down: down,
                // Never read here; the listener's own tracker is the only
                // consumer of this field.
                changed_modifier: None,
            };
            let mut out = self.latch.guard(&event);
            if let Some(edge) = edge_from(&event) {
                out.extend(self.latch.apply(&edge, &self.dictate, self.paused));
            }
            out
        }
    }

    const CTRL: HkModifiers = HkModifiers::CTRL_LEFT;
    const CTRL_SHIFT: HkModifiers = HkModifiers::CTRL_LEFT.union(HkModifiers::SHIFT_LEFT);
    const NOTHING: HkModifiers = HkModifiers::empty();

    fn ctrl_keyboard() -> Keyboard {
        Keyboard::new(&[bare(Modifiers::CTRL_LEFT)])
    }

    /// The bug, in one test. Without the guard the `C` has no `Edge` and is
    /// simply dropped, the Control release stops the recording, and half a
    /// second of room noise is transcribed into the user's document.
    #[test]
    fn a_keystroke_under_a_held_bare_modifier_abandons_the_hold() {
        let mut kb = ctrl_keyboard();
        assert_eq!(kb.modifiers(CTRL, true), vec![press(Binding::Dictate)]);
        assert_eq!(kb.key(CTRL, HkKey::C, true), vec![abort(Binding::Dictate)]);
    }

    /// The headline Windows scenario, end to end. `Ctrl+C` then `Ctrl+V`
    /// inside the lock window is read by the state machine as a double tap,
    /// which locks hands-free recording — a silent, open microphone after the
    /// most common key sequence on the platform.
    ///
    /// What has to come out the other side: no `Released` at all, so nothing
    /// is ever transcribed, and each press answered by an abort, so the
    /// machine is back in `Idle` before the next one can pair with it.
    #[test]
    fn copy_then_paste_can_never_lock_hands_free_recording() {
        let mut kb = ctrl_keyboard();
        let mut seen = Vec::new();
        for (modifiers, key, down) in [
            (CTRL, None, true),           // Ctrl down
            (CTRL, Some(HkKey::C), true), // C
            (CTRL, Some(HkKey::C), false),
            (NOTHING, None, false),       // Ctrl up
            (CTRL, None, true),           // Ctrl down again
            (CTRL, Some(HkKey::V), true), // V
            (CTRL, Some(HkKey::V), false),
            (NOTHING, None, false), // Ctrl up
        ] {
            seen.extend(kb.feed(modifiers, key, down));
        }

        // The two presses are real — each is a fresh physical Control — but
        // both are cancelled before the machine can pair them into a tap.
        assert_eq!(
            seen,
            vec![
                press(Binding::Dictate),
                abort(Binding::Dictate),
                press(Binding::Dictate),
                abort(Binding::Dictate),
            ]
        );
    }

    /// Neither the key-up nor the modifier-up may report anything, or the
    /// pipeline would stop a recording it already abandoned, or start a new
    /// one on the way out of the shortcut.
    #[test]
    fn an_abandoned_hold_stays_silent_until_the_user_presses_again() {
        let mut kb = ctrl_keyboard();
        kb.modifiers(CTRL, true);
        kb.key(CTRL, HkKey::C, true);

        assert!(kb.key(CTRL, HkKey::C, false).is_empty());
        assert!(kb.modifiers(NOTHING, false).is_empty());

        // ...and the hotkey is not dead, just disarmed for that one hold.
        assert_eq!(kb.modifiers(CTRL, true), vec![press(Binding::Dictate)]);
    }

    /// The disarm has to be a real piece of state, not a side effect of the
    /// latch being empty: with Control still down, any other modifier moving
    /// re-satisfies a bare-Control binding and would start a dictation nobody
    /// asked for.
    #[test]
    fn another_modifier_moving_cannot_revive_an_abandoned_hold() {
        let mut kb = ctrl_keyboard();
        kb.modifiers(CTRL, true);
        assert_eq!(kb.key(CTRL, HkKey::C, true), vec![abort(Binding::Dictate)]);

        assert!(kb.modifiers(CTRL_SHIFT, true).is_empty());
        assert!(kb.modifiers(CTRL, false).is_empty());
        assert!(kb.key(CTRL, HkKey::V, true).is_empty());
    }

    /// Key auto-repeat delivers the same key-down over and over. Only the
    /// first may be reported, or the pipeline sees a stream of aborts.
    #[test]
    fn auto_repeat_reports_the_abort_once() {
        let mut kb = ctrl_keyboard();
        kb.modifiers(CTRL, true);
        assert_eq!(kb.key(CTRL, HkKey::C, true), vec![abort(Binding::Dictate)]);
        assert!(kb.key(CTRL, HkKey::C, true).is_empty());
        assert!(kb.key(CTRL, HkKey::C, true).is_empty());
    }

    /// The pipeline's abort path calls `HotkeyBackend::reset`, and a chord
    /// abort *is* an abort — so the cleanup runs on every guard firing. If
    /// reset cleared the disarm it would hand the still-held Control straight
    /// back, and the guard would close the hole and reopen it in one breath.
    #[test]
    fn the_pipelines_reset_does_not_re_arm_a_held_modifier() {
        let mut kb = ctrl_keyboard();
        kb.modifiers(CTRL, true);
        assert_eq!(kb.key(CTRL, HkKey::C, true), vec![abort(Binding::Dictate)]);

        kb.latch.reset();

        assert!(
            kb.modifiers(CTRL_SHIFT, true).is_empty(),
            "still held, so still off"
        );
        assert!(kb.modifiers(CTRL, false).is_empty());
        assert!(kb.key(CTRL, HkKey::C, false).is_empty());
        assert!(kb.modifiers(NOTHING, false).is_empty());

        // Only a genuinely new press brings it back.
        assert_eq!(kb.modifiers(CTRL, true), vec![press(Binding::Dictate)]);
    }

    /// A binding with a trigger key cannot be typed by accident, so the guard
    /// has no business touching it — including when the user types under it.
    #[test]
    fn a_binding_with_a_trigger_key_is_left_alone() {
        let mut kb = Keyboard::new(&[Hotkey {
            modifiers: Modifiers::CTRL_LEFT,
            key: Some(TriggerKey::Space),
        }]);
        assert!(kb.modifiers(CTRL, true).is_empty());
        assert_eq!(
            kb.key(CTRL, HkKey::Space, true),
            vec![press(Binding::Dictate)]
        );

        assert!(kb.key(CTRL, HkKey::C, true).is_empty(), "not guarded");
        assert_eq!(
            kb.key(CTRL, HkKey::Space, false),
            vec![release(Binding::Dictate)]
        );
    }

    /// A two-modifier binding is guarded — `Ctrl+Shift+S` is as real as
    /// `Ctrl+C` — but the guard costs it nothing until a third key lands.
    #[test]
    fn a_two_modifier_binding_still_starts_and_stops_normally() {
        let mut kb = Keyboard::new(&[bare(Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT)]);
        assert!(
            kb.modifiers(CTRL, true).is_empty(),
            "half a chord is not a chord"
        );
        assert_eq!(
            kb.modifiers(CTRL_SHIFT, true),
            vec![press(Binding::Dictate)]
        );
        assert_eq!(kb.modifiers(CTRL, false), vec![release(Binding::Dictate)]);
    }

    #[test]
    fn a_two_modifier_binding_is_guarded_too() {
        let mut kb = Keyboard::new(&[bare(Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT)]);
        kb.modifiers(CTRL, true);
        kb.modifiers(CTRL_SHIFT, true);
        assert_eq!(
            kb.key(CTRL_SHIFT, HkKey::S, true),
            vec![abort(Binding::Dictate)]
        );
    }

    /// Hands-free, mechanically: the modifier is not held, so there is no hold
    /// to abandon and a `Ctrl+C` is somebody else's shortcut. The pipeline
    /// gates on the state machine as well, for the instant during a locking
    /// press when the key genuinely is still down.
    #[test]
    fn a_shortcut_with_nothing_held_is_none_of_the_guards_business() {
        let mut kb = ctrl_keyboard();
        kb.modifiers(CTRL, true);
        assert_eq!(
            kb.modifiers(NOTHING, false),
            vec![release(Binding::Dictate)]
        );

        assert!(kb.key(NOTHING, HkKey::C, true).is_empty());
        assert!(kb.key(NOTHING, HkKey::C, false).is_empty());
    }

    /// The guard ignores the pause flag, for the same reason releases do: a
    /// hold that started before the pause must still be able to end, and
    /// ending it by throwing the take away is the safe direction.
    #[test]
    fn a_pause_mid_hold_does_not_disable_the_guard() {
        let mut kb = ctrl_keyboard();
        kb.modifiers(CTRL, true);
        kb.paused = true;
        assert_eq!(kb.key(CTRL, HkKey::C, true), vec![abort(Binding::Dictate)]);
    }

    /// A lock key is not a keystroke: `is_key_down` carries its lamp, so a
    /// hold taken with Caps Lock already on would abort on the next event.
    #[test]
    fn a_lock_key_does_not_abandon_the_hold() {
        let mut kb = ctrl_keyboard();
        kb.modifiers(CTRL, true);
        assert!(kb.key(CTRL, HkKey::CapsLock, true).is_empty());
        assert_eq!(
            kb.modifiers(NOTHING, false),
            vec![release(Binding::Dictate)]
        );
    }
}
