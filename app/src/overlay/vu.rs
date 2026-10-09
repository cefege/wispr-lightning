//! The recording pill's level meter as pure arithmetic (v2-ui-spec §1.4).
//!
//! The strip is a scrolling history, not a spectrum: bar `i` holds the `i`-th
//! entry of a rolling window of the last 18 samples, oldest on the left. Two
//! arrays, deliberately: `targets` scrolls, `displayed` does not. Each bar
//! position runs its own moving average towards whatever target has scrolled
//! into it, which gives the band its trailing feel rather than the hard step
//! of a shift register.

/// Bars in the strip. 18 × 3px + 17 × 2px gaps = the 88px strip width.
pub const BAR_COUNT: usize = 18;

/// Bar height at silence, and the resting height the strip resets to.
pub const BAR_MIN_HEIGHT: f64 = 3.0;

/// Bar height at level 1.0.
pub const BAR_MAX_HEIGHT: f64 = 20.0;

/// Per-bar moving-average weight, tuned against a 25 Hz feed.
const SMOOTHING: f64 = 0.5;

/// Map a 0–1 dB-linear level onto the 0–1 the bars are drawn from. The square
/// root lifts quiet speech (~0.1) off the baseline. Non-finite input is 0: a
/// single NaN in the moving average would freeze the band for the rest of the
/// recording.
fn curve(level: f32) -> f64 {
    let level = f64::from(level);
    if !level.is_finite() {
        return 0.0;
    }
    level.clamp(0.0, 1.0).sqrt()
}

#[derive(Debug, Clone, PartialEq)]
pub struct LevelMeter {
    targets: [f64; BAR_COUNT],
    displayed: [f64; BAR_COUNT],
    /// Current bar heights in px, left to right, rounded to a tenth: below
    /// that the difference is sub-pixel and only invalidates layout.
    heights: [f64; BAR_COUNT],
}

impl Default for LevelMeter {
    fn default() -> Self {
        Self {
            targets: [0.0; BAR_COUNT],
            displayed: [0.0; BAR_COUNT],
            heights: [BAR_MIN_HEIGHT; BAR_COUNT],
        }
    }
}

impl LevelMeter {
    pub fn heights(&self) -> &[f64; BAR_COUNT] {
        &self.heights
    }

    /// Accept one level sample and advance every bar one step.
    pub fn push(&mut self, level: f32) {
        self.targets.copy_within(1.., 0);
        self.targets[BAR_COUNT - 1] = curve(level);
        for i in 0..BAR_COUNT {
            let smoothed = self.displayed[i] * SMOOTHING + self.targets[i] * (1.0 - SMOOTHING);
            self.displayed[i] = smoothed;
            let height = BAR_MIN_HEIGHT + smoothed * (BAR_MAX_HEIGHT - BAR_MIN_HEIGHT);
            self.heights[i] = (height * 10.0).round() / 10.0;
        }
    }

    /// Return to silence: history and filter cleared, all bars at rest.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_sample_enters_on_the_right_and_scrolls_left() {
        let mut meter = LevelMeter::default();
        meter.push(1.0);
        // Half-way there after one step of an even moving average.
        assert_eq!(meter.heights()[BAR_COUNT - 1], 11.5);
        assert_eq!(meter.heights()[BAR_COUNT - 2], BAR_MIN_HEIGHT);

        meter.push(0.0);
        // The loud sample moved one bar left; the newest bar decays.
        assert!(meter.heights()[BAR_COUNT - 2] > BAR_MIN_HEIGHT);
        assert!(meter.heights()[BAR_COUNT - 1] < 11.5);
    }

    #[test]
    fn non_finite_levels_do_not_poison_the_band() {
        let mut meter = LevelMeter::default();
        meter.push(f32::NAN);
        meter.push(f32::INFINITY);
        meter.push(0.25);
        assert!(meter.heights().iter().all(|h| h.is_finite()));
        // sqrt(0.25) = 0.5, half of it after one step: 3 + 0.25 * 17.
        assert_eq!(meter.heights()[BAR_COUNT - 1], 7.3);
    }

    #[test]
    fn reset_returns_every_bar_to_rest() {
        let mut meter = LevelMeter::default();
        for _ in 0..5 {
            meter.push(0.8);
        }
        meter.reset();
        assert_eq!(meter.heights(), &[BAR_MIN_HEIGHT; BAR_COUNT]);
        meter.push(0.0);
        assert_eq!(meter.heights(), &[BAR_MIN_HEIGHT; BAR_COUNT]);
    }
}
