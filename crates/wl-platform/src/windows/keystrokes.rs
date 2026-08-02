//! Pure keystroke shaping for natural mode.
//!
//! Two things in the Swift `TextInjector` are user-visible speed
//! characteristics rather than implementation details, so they are modelled
//! here where they can be tested without a keyboard: the UTF-16 expansion that
//! `KEYEVENTF_UNICODE` requires, and the timing envelope
//! (`1/cps × uniform(0.6, 1.4)` between characters, `uniform(30 ms, 80 ms)`
//! holding each key).

use std::time::Duration;

/// Uniform jitter applied to the base inter-key delay: ±40 %.
pub(crate) const JITTER_MIN: f64 = 0.6;
pub(crate) const JITTER_MAX: f64 = 1.4;

/// Key hold time, down to up. The Swift comment explains the lower bound:
/// "ensures fast-key detectors register a press, not a glitch".
pub(crate) const HOLD_MIN: Duration = Duration::from_millis(30);
pub(crate) const HOLD_MAX: Duration = Duration::from_millis(80);

/// Below this the pacing maths stops meaning anything; clamp instead of
/// dividing by zero when a settings file carries a nonsense speed.
const MIN_CHARS_PER_SECOND: f64 = 0.1;

/// UTF-16 code units for `s`, in the order `SendInput` must receive them.
///
/// `KEYEVENTF_UNICODE` carries one UTF-16 code unit per `INPUT`, so a
/// non-BMP character (emoji, historic scripts) becomes a surrogate *pair*.
/// Windows only composes the pair if both halves arrive adjacently in the same
/// `SendInput` batch, which is why callers drive the injection off this list
/// rather than off `chars()`.
pub(crate) fn to_utf16_events(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Deterministic jitter source.
///
/// `rand` is not in this crate's dependency tree and pulling it in for two
/// uniform draws that only need to look human would be silly; xorshift64* is
/// a handful of instructions with a period long enough that no dictation will
/// ever see it repeat. Being seeded and reproducible also lets the tests pin
/// the bounds instead of hoping.
pub(crate) struct Pacer {
    state: u64,
}

impl Pacer {
    pub(crate) fn new(seed: u64) -> Self {
        // xorshift64* is dead at zero, so a zero seed must not survive.
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    /// A fresh pacer seeded from the clock, for production use.
    // Only the injector calls this, and the injector is Windows-only; the
    // host test shim compiles this module on its own.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn from_clock() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5DEE_CE66);
        Self::new(nanos)
    }

    /// Next draw in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let value = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // 53 bits is exactly the f64 mantissa, so this is uniform and never 1.0.
        (value >> 11) as f64 / (1u64 << 53) as f64
    }

    fn between(&mut self, low: f64, high: f64) -> f64 {
        low + self.unit() * (high - low)
    }

    /// Delay before the next character.
    pub(crate) fn delay(&mut self, chars_per_second: f64) -> Duration {
        let cps = chars_per_second.max(MIN_CHARS_PER_SECOND);
        Duration::from_secs_f64(self.between(JITTER_MIN, JITTER_MAX) / cps)
    }

    /// How long to hold one key down.
    pub(crate) fn hold(&mut self) -> Duration {
        let low = HOLD_MIN.as_secs_f64();
        let high = HOLD_MAX.as_secs_f64();
        Duration::from_secs_f64(self.between(low, high))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_produces_one_event_per_character() {
        assert_eq!(to_utf16_events("hi"), vec![0x0068, 0x0069]);
    }

    #[test]
    fn an_astral_plane_character_becomes_an_adjacent_surrogate_pair() {
        // U+1F600 GRINNING FACE.
        let events = to_utf16_events("😀");
        assert_eq!(events, vec![0xD83D, 0xDE00]);
        assert!(
            (0xD800..0xDC00).contains(&events[0]),
            "high surrogate first"
        );
        assert!(
            (0xDC00..0xE000).contains(&events[1]),
            "low surrogate second"
        );
    }

    #[test]
    fn surrogate_pairs_stay_adjacent_inside_a_longer_string() {
        let events = to_utf16_events("a😀b");
        assert_eq!(events, vec![0x0061, 0xD83D, 0xDE00, 0x0062]);
    }

    #[test]
    fn the_event_stream_round_trips_back_to_the_original_text() {
        for text in ["", "plain", "héllo — naïve", "😀🇬🇧 mixed 汉字"] {
            let events = to_utf16_events(text);
            assert_eq!(String::from_utf16(&events).ok().as_deref(), Some(text));
        }
    }

    #[test]
    fn inter_key_delay_stays_inside_the_forty_percent_jitter_band() {
        let mut pacer = Pacer::new(1);
        let base = 1.0 / 4.0; // "normal" preset: 4 chars/second.
        let (mut low, mut high) = (f64::MAX, 0.0f64);
        for _ in 0..10_000 {
            let secs = pacer.delay(4.0).as_secs_f64();
            assert!(
                secs >= base * JITTER_MIN - f64::EPSILON && secs <= base * JITTER_MAX,
                "delay {secs} outside band"
            );
            low = low.min(secs);
            high = high.max(secs);
        }
        // A constant or barely-varying delay would still satisfy the bounds
        // above, so require the draw to actually cover the band.
        assert!(low < base * 0.62, "jitter never approaches the lower bound");
        assert!(
            high > base * 1.38,
            "jitter never approaches the upper bound"
        );
    }

    #[test]
    fn a_faster_preset_types_proportionally_faster() {
        let mut slow = Pacer::new(7);
        let mut expert = Pacer::new(7);
        // Same seed, so the same draw: the only difference is the rate.
        let slow_delay = slow.delay(2.5).as_secs_f64();
        let expert_delay = expert.delay(6.5).as_secs_f64();
        let ratio = slow_delay / expert_delay;
        // `Duration` is nanosecond-quantised, so this is not exact.
        assert!((ratio - 6.5 / 2.5).abs() < 1e-6, "ratio was {ratio}");
    }

    #[test]
    fn a_nonsense_speed_cannot_produce_an_infinite_delay() {
        let mut pacer = Pacer::new(3);
        for cps in [0.0, -5.0, f64::NAN] {
            let delay = pacer.delay(cps);
            assert!(delay.as_secs_f64().is_finite());
            assert!(delay <= Duration::from_secs(30));
        }
    }

    #[test]
    fn key_hold_time_stays_between_thirty_and_eighty_milliseconds() {
        let mut pacer = Pacer::new(42);
        let (mut low, mut high) = (Duration::MAX, Duration::ZERO);
        for _ in 0..10_000 {
            let hold = pacer.hold();
            assert!(
                hold >= HOLD_MIN && hold <= HOLD_MAX,
                "hold {hold:?} out of range"
            );
            low = low.min(hold);
            high = high.max(hold);
        }
        assert!(low < Duration::from_millis(32));
        assert!(high > Duration::from_millis(78));
    }

    #[test]
    fn a_zero_seed_still_produces_varying_draws() {
        let mut pacer = Pacer::new(0);
        let first = pacer.unit();
        assert!(
            (0..20).any(|_| (pacer.unit() - first).abs() > 1e-9),
            "generator collapsed to a fixed point"
        );
    }
}
