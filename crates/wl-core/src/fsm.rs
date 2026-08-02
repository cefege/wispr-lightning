//! The push-to-talk recording state machine.
//!
//! Pure: every input is an event plus a timestamp, every output is a list of
//! actions. No timers, no threads, no OS. That is what makes the tap-vs-hold
//! timing — the single most user-visible behavior in the app — actually
//! testable.
//!
//! The interaction model, which is subtle and must be preserved exactly. The
//! user picks one of three press behaviours; the parts they share come first:
//!
//! - Press from idle starts recording in *listening* (push-to-talk) mode.
//! - A **second press within 0.5 s of the first** locks into hands-free mode;
//!   key release then does nothing and a third press stops.
//! - Releasing after holding for **>= 0.5 s** stops recording 0.5 s later, a
//!   trailing buffer that captures the tail of the user's speech. A long hold
//!   is push-to-talk in every behaviour.
//!
//! What differs is the *quick tap* — a release less than 0.5 s after the
//! press ([`PressBehavior`]):
//!
//! - `Legacy`: stops recording at exactly 0.5 s *after the first press* — not
//!   0.5 s after the release — so the lock window is a fixed wall-clock window
//!   rather than a sliding one, and a second tap can still arrive.
//! - `Hold`: stops immediately, with no trailing buffer at all. The asymmetry
//!   with a genuine hold is deliberate: the user let go early, so there is no
//!   tail to capture.
//! - `TapToToggle`: locks hands-free straight away; the next press stops.

use std::time::{Duration, Instant};

/// Second press within this window of the first locks hands-free mode.
pub const LOCK_DEBOUNCE: Duration = Duration::from_millis(500);
/// Delay between a push-to-talk release and the actual stop.
pub const TRAILING_BUFFER: Duration = Duration::from_millis(500);

/// What a quick tap of the hotkey means. The user picks this in Settings
/// (`hotkeyPressBehavior`).
///
/// Passed to [`Machine::handle`] per event rather than stored, so a change
/// made in the settings window mid-session takes effect on the very next
/// press instead of the next launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PressBehavior {
    /// Recording lasts as long as the key is held; releasing always ends it.
    Hold,
    /// Press once to start, press again to stop. Holding is still
    /// push-to-talk.
    TapToToggle,
    /// Quick tap waits for a second tap to lock hands-free; hold longer than
    /// ~0.5 s for push-to-talk.
    #[default]
    Legacy,
}

impl PressBehavior {
    /// Parse a `hotkeyPressBehavior` settings value.
    ///
    /// Anything unrecognised is `Legacy`, which is what the Swift `switch`
    /// does with its `default:` arm — an unknown value must not leave the
    /// hotkey inert.
    pub fn from_setting(raw: &str) -> Self {
        match raw {
            "hold" => Self::Hold,
            // `"tapToToggle"` never comes from the Swift settings window, but
            // it is the obvious spelling and has appeared in hand-written
            // files; accept it rather than silently demoting to legacy.
            "toggle" | "tapToToggle" => Self::TapToToggle,
            _ => Self::Legacy,
        }
    }

    /// The canonical settings value. Always one of the three tags the Swift
    /// picker writes, because a Swift build reading anything else falls
    /// through to legacy.
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::TapToToggle => "toggle",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    /// Push-to-talk: recording lasts as long as the key is held.
    Listening {
        last_press: Instant,
    },
    /// Hands-free: recording continues until the next press.
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Press(Instant),
    Release(Instant),
    /// The delayed-stop timer previously requested via [`Action::ScheduleStop`].
    StopTimerFired,
    /// System sleep or another hard interrupt: drop the recording entirely.
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    StartRecording,
    StopRecording,
    /// Discard the in-flight recording without transcribing it.
    AbortRecording,
    /// Switch the overlay to its hands-free presentation.
    ShowLocked,
    ScheduleStop(Duration),
    CancelScheduledStop,
}

#[derive(Debug)]
pub struct Machine {
    state: State,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self { state: State::Idle }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_recording(&self) -> bool {
        self.state != State::Idle
    }

    /// Apply `event` under `behavior`, returning the actions the host must
    /// perform in order.
    ///
    /// `behavior` only ever affects a quick-tap release; every other
    /// transition is shared, exactly as in the Swift original where
    /// `onHotkeyPress` never consults the setting at all.
    pub fn handle(&mut self, event: Event, behavior: PressBehavior) -> Vec<Action> {
        match (self.state, event) {
            // -- Press ------------------------------------------------------
            (State::Idle, Event::Press(now)) => {
                self.state = State::Listening { last_press: now };
                vec![Action::StartRecording]
            }
            (State::Listening { last_press }, Event::Press(now)) => {
                if now.duration_since(last_press) < LOCK_DEBOUNCE {
                    self.state = State::Locked;
                    vec![Action::CancelScheduledStop, Action::ShowLocked]
                } else {
                    self.stop()
                }
            }
            (State::Locked, Event::Press(_)) => self.stop(),

            // -- Release ----------------------------------------------------
            (State::Listening { last_press }, Event::Release(now)) => {
                let held = now.duration_since(last_press);
                if held >= LOCK_DEBOUNCE {
                    // Genuine hold: push-to-talk in all three behaviours, with
                    // a trailing buffer so the tail of the sentence survives.
                    return vec![
                        Action::CancelScheduledStop,
                        Action::ScheduleStop(TRAILING_BUFFER),
                    ];
                }
                match behavior {
                    // The user let go early and meant it. No trailing buffer:
                    // there is no tail, and waiting would feel broken.
                    PressBehavior::Hold => self.stop(),
                    PressBehavior::TapToToggle => {
                        self.state = State::Locked;
                        vec![Action::CancelScheduledStop, Action::ShowLocked]
                    }
                    // Keep the lock window open until 0.5 s after the *press*,
                    // so a second tap can still arrive.
                    PressBehavior::Legacy => vec![
                        Action::CancelScheduledStop,
                        Action::ScheduleStop(LOCK_DEBOUNCE - held),
                    ],
                }
            }
            // In hands-free mode the key release is meaningless.
            (State::Locked | State::Idle, Event::Release(_)) => Vec::new(),

            // -- Timer ------------------------------------------------------
            (State::Listening { .. }, Event::StopTimerFired) => self.stop(),
            // A stale timer from a session that already ended, or one that
            // fired after the user locked. Both must be ignored.
            (State::Locked | State::Idle, Event::StopTimerFired) => Vec::new(),

            // -- Abort ------------------------------------------------------
            (State::Idle, Event::Abort) => Vec::new(),
            (_, Event::Abort) => {
                self.state = State::Idle;
                vec![Action::CancelScheduledStop, Action::AbortRecording]
            }
        }
    }

    fn stop(&mut self) -> Vec<Action> {
        self.state = State::Idle;
        vec![Action::CancelScheduledStop, Action::StopRecording]
    }
}

/// Tracks the elapsed-time warnings shown during a long recording.
///
/// Monotonic: once escalated it never steps back down within a session, so a
/// user who crosses 570 s never sees the milder 540 s warning again.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WarningState(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// Keep recording; `elapsed_label` is `Some` once the timer becomes
    /// visible (the overlay hides it for the first 30 seconds).
    Continue { warning: WarningState },
    /// The hard duration cap was reached.
    AutoStop,
}

impl WarningState {
    pub fn level(self) -> u8 {
        self.0
    }

    pub fn reset(&mut self) {
        self.0 = 0;
    }

    /// Advance one second of recording. `elapsed` is whole seconds since the
    /// recording started.
    pub fn tick(&mut self, elapsed: u64) -> Tick {
        use crate::consts::{FINAL_WARNING_SECS, MAX_RECORDING_SECS, WARNING_SECS};
        if elapsed >= MAX_RECORDING_SECS {
            return Tick::AutoStop;
        }
        if elapsed >= FINAL_WARNING_SECS {
            self.0 = 2;
        } else if elapsed >= WARNING_SECS {
            self.0 = self.0.max(1);
        }
        Tick::Continue { warning: *self }
    }
}

/// The overlay keeps the elapsed timer hidden for the first 30 seconds so brief
/// dictations get a minimal pill.
pub fn elapsed_label(elapsed: u64, warning: WarningState) -> Option<String> {
    if elapsed < 30 {
        return None;
    }
    let base = format!("{}:{:02}", elapsed / 60, elapsed % 60);
    Some(if warning.level() > 0 {
        format!("{base} \u{26A0}\u{FE0F}")
    } else {
        base
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }
    fn after(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// The behaviour the pre-B-015 machine hard-coded. Every test below that
    /// predates the picker passes it explicitly, so "legacy is unchanged" is
    /// asserted by the same assertions that always asserted it.
    const LEGACY: PressBehavior = PressBehavior::Legacy;

    #[test]
    fn press_from_idle_starts_recording() {
        let mut m = Machine::new();
        let t = t0();
        assert_eq!(
            m.handle(Event::Press(t), LEGACY),
            vec![Action::StartRecording]
        );
        assert_eq!(m.state(), State::Listening { last_press: t });
        assert!(m.is_recording());
    }

    #[test]
    fn hold_then_release_schedules_the_trailing_buffer() {
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        let acts = m.handle(Event::Release(after(t, 2_000)), LEGACY);
        assert_eq!(
            acts,
            vec![
                Action::CancelScheduledStop,
                Action::ScheduleStop(TRAILING_BUFFER)
            ]
        );
        // Still recording until the timer fires — this is the tail-capture window.
        assert!(m.is_recording());
        assert_eq!(
            m.handle(Event::StopTimerFired, LEGACY),
            vec![Action::CancelScheduledStop, Action::StopRecording]
        );
        assert_eq!(m.state(), State::Idle);
    }

    #[test]
    fn release_at_exactly_the_threshold_counts_as_a_hold() {
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        let acts = m.handle(Event::Release(after(t, 500)), LEGACY);
        assert_eq!(acts[1], Action::ScheduleStop(TRAILING_BUFFER));
    }

    #[test]
    fn quick_tap_stops_half_a_second_after_the_press_not_the_release() {
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        // Released 120 ms in: the remaining lock window is 380 ms.
        let acts = m.handle(Event::Release(after(t, 120)), LEGACY);
        assert_eq!(
            acts[1],
            Action::ScheduleStop(Duration::from_millis(380)),
            "the lock window is anchored to the press, not the release"
        );
    }

    #[test]
    fn double_tap_locks_hands_free_mode() {
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        m.handle(Event::Release(after(t, 80)), LEGACY);
        let acts = m.handle(Event::Press(after(t, 300)), LEGACY);
        assert_eq!(acts, vec![Action::CancelScheduledStop, Action::ShowLocked]);
        assert_eq!(m.state(), State::Locked);
    }

    #[test]
    fn release_is_ignored_while_locked() {
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        m.handle(Event::Press(after(t, 100)), LEGACY);
        assert_eq!(m.state(), State::Locked);
        assert!(m.handle(Event::Release(after(t, 150)), LEGACY).is_empty());
        assert_eq!(m.state(), State::Locked, "release must not end hands-free");
    }

    #[test]
    fn third_press_stops_hands_free_recording() {
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        m.handle(Event::Press(after(t, 100)), LEGACY);
        assert_eq!(
            m.handle(Event::Press(after(t, 9_000)), LEGACY),
            vec![Action::CancelScheduledStop, Action::StopRecording]
        );
        assert_eq!(m.state(), State::Idle);
    }

    #[test]
    fn slow_second_press_stops_instead_of_locking() {
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        // 600 ms > the 500 ms lock window, so this is a stop, not a lock.
        assert_eq!(
            m.handle(Event::Press(after(t, 600)), LEGACY),
            vec![Action::CancelScheduledStop, Action::StopRecording]
        );
        assert_eq!(m.state(), State::Idle);
    }

    #[test]
    fn a_stale_stop_timer_cannot_end_a_locked_session() {
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        m.handle(Event::Release(after(t, 100)), LEGACY);
        m.handle(Event::Press(after(t, 200)), LEGACY); // locks
        assert!(m.handle(Event::StopTimerFired, LEGACY).is_empty());
        assert_eq!(m.state(), State::Locked);
    }

    #[test]
    fn a_stop_timer_firing_when_idle_is_a_no_op() {
        let mut m = Machine::new();
        assert!(m.handle(Event::StopTimerFired, LEGACY).is_empty());
        assert_eq!(m.state(), State::Idle);
    }

    #[test]
    fn abort_discards_an_active_recording() {
        for lock in [false, true] {
            let mut m = Machine::new();
            let t = t0();
            m.handle(Event::Press(t), LEGACY);
            if lock {
                m.handle(Event::Press(after(t, 100)), LEGACY);
            }
            assert_eq!(
                m.handle(Event::Abort, LEGACY),
                vec![Action::CancelScheduledStop, Action::AbortRecording]
            );
            assert_eq!(m.state(), State::Idle);
        }
    }

    #[test]
    fn abort_while_idle_does_nothing() {
        assert!(Machine::new().handle(Event::Abort, LEGACY).is_empty());
    }

    #[test]
    fn a_lock_resets_the_press_anchor_so_the_next_press_stops() {
        // Press, quick second press to lock, then a third press 400 ms later.
        // If the anchor were not reset the third press would be < 500 ms from
        // the *second* and could be mistaken for another lock.
        let mut m = Machine::new();
        let t = t0();
        m.handle(Event::Press(t), LEGACY);
        m.handle(Event::Press(after(t, 100)), LEGACY);
        assert_eq!(
            m.handle(Event::Press(after(t, 400)), LEGACY),
            vec![Action::CancelScheduledStop, Action::StopRecording]
        );
    }

    // -- the three press behaviours ----------------------------------------

    /// A deterministic clock. `Instant` has no public constructor, so a test
    /// fixes one origin and offsets it: every timing below is exact, and
    /// nothing sleeps or reads the wall clock twice.
    struct Clock(Instant);

    impl Clock {
        fn new() -> Self {
            Self(Instant::now())
        }
        fn at(&self, ms: u64) -> Instant {
            self.0 + Duration::from_millis(ms)
        }
    }

    /// A machine already in `state`, driven there through the press path that
    /// all three behaviours share.
    fn start_in(state: State, c: &Clock) -> Machine {
        let mut m = Machine::new();
        match state {
            State::Idle => {}
            State::Listening { .. } => {
                m.handle(Event::Press(c.at(0)), LEGACY);
            }
            State::Locked => {
                m.handle(Event::Press(c.at(0)), LEGACY);
                // A second press inside the lock window locks in every
                // behaviour: `onHotkeyPress` never reads the setting.
                m.handle(Event::Press(c.at(100)), LEGACY);
            }
        }
        assert_eq!(m.state(), state, "test setup did not reach {state:?}");
        m
    }

    /// Assert the complete state x event table for one behaviour.
    ///
    /// Only the quick-tap release differs between behaviours, so it is the
    /// only parameter — anything else diverging is a regression, and this
    /// table is what catches it.
    fn assert_transition_table(
        behavior: PressBehavior,
        c: &Clock,
        quick_tap: &[Action],
        after_quick_tap: State,
    ) {
        let listening = State::Listening {
            last_press: c.at(0),
        };
        let hold = [
            Action::CancelScheduledStop,
            Action::ScheduleStop(TRAILING_BUFFER),
        ];
        let stop = [Action::CancelScheduledStop, Action::StopRecording];
        let abort = [Action::CancelScheduledStop, Action::AbortRecording];
        let lock = [Action::CancelScheduledStop, Action::ShowLocked];

        let table: &[(State, Event, &[Action], State)] = &[
            // -- from idle
            (
                State::Idle,
                Event::Press(c.at(0)),
                &[Action::StartRecording],
                listening,
            ),
            (State::Idle, Event::Release(c.at(0)), &[], State::Idle),
            (State::Idle, Event::StopTimerFired, &[], State::Idle),
            (State::Idle, Event::Abort, &[], State::Idle),
            // -- from listening
            (listening, Event::Press(c.at(100)), &lock, State::Locked),
            (listening, Event::Press(c.at(600)), &stop, State::Idle),
            (
                listening,
                Event::Release(c.at(120)),
                quick_tap,
                after_quick_tap,
            ),
            // A release at exactly the threshold is a hold, not a tap.
            (listening, Event::Release(c.at(500)), &hold, listening),
            (listening, Event::Release(c.at(2_000)), &hold, listening),
            (listening, Event::StopTimerFired, &stop, State::Idle),
            (listening, Event::Abort, &abort, State::Idle),
            // -- from locked
            (State::Locked, Event::Press(c.at(9_000)), &stop, State::Idle),
            (State::Locked, Event::Release(c.at(150)), &[], State::Locked),
            (State::Locked, Event::StopTimerFired, &[], State::Locked),
            (State::Locked, Event::Abort, &abort, State::Idle),
        ];

        for (start, event, expected, next) in table {
            let mut m = start_in(*start, c);
            assert_eq!(
                m.handle(*event, behavior),
                *expected,
                "{behavior:?}: {start:?} + {event:?}"
            );
            assert_eq!(m.state(), *next, "{behavior:?}: {start:?} + {event:?}");
        }
    }

    #[test]
    fn legacy_transition_table() {
        let c = Clock::new();
        assert_transition_table(
            PressBehavior::Legacy,
            &c,
            // Stop 0.5 s after the *press*, leaving 380 ms for a second tap.
            &[
                Action::CancelScheduledStop,
                Action::ScheduleStop(Duration::from_millis(380)),
            ],
            State::Listening {
                last_press: c.at(0),
            },
        );
    }

    #[test]
    fn hold_transition_table() {
        let c = Clock::new();
        assert_transition_table(
            PressBehavior::Hold,
            &c,
            // Releasing always ends it, and a quick tap gets no trailing
            // buffer at all — the one asymmetry with a genuine hold.
            &[Action::CancelScheduledStop, Action::StopRecording],
            State::Idle,
        );
    }

    #[test]
    fn tap_to_toggle_transition_table() {
        let c = Clock::new();
        assert_transition_table(
            PressBehavior::TapToToggle,
            &c,
            &[Action::CancelScheduledStop, Action::ShowLocked],
            State::Locked,
        );
    }

    #[test]
    fn tap_to_toggle_locks_on_the_tap_and_stops_on_the_next_press() {
        let c = Clock::new();
        let mut m = Machine::new();
        m.handle(Event::Press(c.at(0)), PressBehavior::TapToToggle);
        m.handle(Event::Release(c.at(80)), PressBehavior::TapToToggle);
        assert_eq!(m.state(), State::Locked);
        // Hands-free: the key is not held, and only a press ends it.
        assert!(m
            .handle(Event::Release(c.at(200)), PressBehavior::TapToToggle)
            .is_empty());
        assert_eq!(
            m.handle(Event::Press(c.at(5_000)), PressBehavior::TapToToggle),
            vec![Action::CancelScheduledStop, Action::StopRecording]
        );
    }

    #[test]
    fn a_long_hold_is_push_to_talk_in_every_behavior() {
        for behavior in [
            PressBehavior::Hold,
            PressBehavior::TapToToggle,
            PressBehavior::Legacy,
        ] {
            let c = Clock::new();
            let mut m = Machine::new();
            m.handle(Event::Press(c.at(0)), behavior);
            assert_eq!(
                m.handle(Event::Release(c.at(1_500)), behavior),
                vec![
                    Action::CancelScheduledStop,
                    Action::ScheduleStop(TRAILING_BUFFER)
                ],
                "{behavior:?} must keep the trailing buffer on a genuine hold"
            );
            assert!(m.is_recording(), "{behavior:?} stopped before the buffer");
        }
    }

    #[test]
    fn a_behavior_change_mid_session_takes_effect_on_the_next_event() {
        // The user switches the picker while the key is still down. The next
        // release must honour the new setting, not the one in force at press.
        let c = Clock::new();
        let mut m = Machine::new();
        m.handle(Event::Press(c.at(0)), PressBehavior::Legacy);
        assert_eq!(
            m.handle(Event::Release(c.at(120)), PressBehavior::Hold),
            vec![Action::CancelScheduledStop, Action::StopRecording]
        );
        assert_eq!(m.state(), State::Idle);
    }

    #[test]
    fn press_behavior_settings_values_match_the_swift_picker_tags() {
        assert_eq!(PressBehavior::from_setting("hold"), PressBehavior::Hold);
        assert_eq!(
            PressBehavior::from_setting("toggle"),
            PressBehavior::TapToToggle
        );
        assert_eq!(
            PressBehavior::from_setting("tapToToggle"),
            PressBehavior::TapToToggle
        );
        assert_eq!(PressBehavior::from_setting("legacy"), PressBehavior::Legacy);
        // Swift's `switch` sends anything else to its `default:` arm.
        assert_eq!(PressBehavior::from_setting(""), PressBehavior::Legacy);
        assert_eq!(
            PressBehavior::from_setting("nonsense"),
            PressBehavior::Legacy
        );

        // Only the three tags Swift writes may ever be written back: it reads
        // this string literally and falls through to legacy on anything else.
        assert_eq!(PressBehavior::Hold.as_setting(), "hold");
        assert_eq!(PressBehavior::TapToToggle.as_setting(), "toggle");
        assert_eq!(PressBehavior::Legacy.as_setting(), "legacy");
        assert_eq!(PressBehavior::default(), PressBehavior::Legacy);
    }

    // -- duration warnings -------------------------------------------------

    #[test]
    fn warnings_escalate_at_the_documented_thresholds() {
        let mut w = WarningState::default();
        assert_eq!(w.tick(539), Tick::Continue { warning: w });
        assert_eq!(w.level(), 0);
        w.tick(540);
        assert_eq!(w.level(), 1);
        w.tick(569);
        assert_eq!(w.level(), 1);
        w.tick(570);
        assert_eq!(w.level(), 2);
    }

    #[test]
    fn warnings_never_step_back_down() {
        let mut w = WarningState::default();
        w.tick(575);
        assert_eq!(w.level(), 2);
        w.tick(100);
        assert_eq!(w.level(), 2, "warning level must be monotonic in a session");
    }

    #[test]
    fn recording_auto_stops_at_the_cap() {
        let mut w = WarningState::default();
        assert_eq!(w.tick(599), Tick::Continue { warning: w });
        assert_eq!(w.tick(600), Tick::AutoStop);
        assert_eq!(w.tick(9_999), Tick::AutoStop);
    }

    #[test]
    fn elapsed_label_is_hidden_for_the_first_thirty_seconds() {
        let none = WarningState::default();
        assert_eq!(elapsed_label(0, none), None);
        assert_eq!(elapsed_label(29, none), None);
        assert_eq!(elapsed_label(30, none).as_deref(), Some("0:30"));
    }

    #[test]
    fn elapsed_label_formats_and_appends_the_warning_glyph() {
        let mut w = WarningState::default();
        assert_eq!(elapsed_label(545, w).as_deref(), Some("9:05"));
        w.tick(545);
        assert_eq!(
            elapsed_label(545, w).as_deref(),
            Some("9:05 \u{26A0}\u{FE0F}")
        );
    }
}
