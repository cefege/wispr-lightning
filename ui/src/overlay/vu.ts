/**
 * The recording pill's level meter, expressed as pure arithmetic.
 *
 * Kept free of the DOM for the same reason `state.ts` is: the numbers here are
 * what parity is measured on (v2-ui-spec §1.4), and a smoothing constant that
 * can only be checked by staring at a moving pill is a constant nobody will
 * ever check. Everything below is a function of its arguments, so the curve,
 * the ring shift, the per-bar filter and the height map can be reasoned about
 * — and driven — without a window.
 *
 * The strip is a *scrolling history*, not a spectrum: bar `i` holds the `i`-th
 * entry of a rolling window of the last 18 samples, oldest on the left, newest
 * on the right. New audio pushes the whole band leftwards.
 */

/** Bars in the strip. 18 × 3px + 17 × 2px gaps = the 88px strip width. */
export const VU_BAR_COUNT = 18;

/** Bar height at silence, and the resting height the strip resets to. */
export const VU_BAR_MIN_HEIGHT = 3;

/** Bar height at level 1.0. */
export const VU_BAR_MAX_HEIGHT = 20;

/**
 * Per-bar exponential moving average weight: an even blend of the previous
 * displayed value and the new target. Tuned in the reference app against a
 * 25 Hz feed; changing the feed rate without changing this changes how twitchy
 * the band looks.
 */
export const VU_SMOOTHING = 0.5;

/**
 * Map a 0–1 normalized RMS level onto the 0–1 the bars are drawn from.
 *
 * The backend already converts RMS to a dB-linear 0–1 (−60 dBFS → 0, 0 dBFS →
 * 1). The square root on top is a mild perceptual curve so that quiet speech,
 * which lands around 0.1, still visibly nudges the bars instead of sitting on
 * the baseline looking like a dead microphone.
 *
 * Non-finite input yields 0 rather than propagating: a single NaN reaching the
 * moving average would poison every subsequent frame, freezing the band for
 * the rest of the recording.
 */
export function curve(level: number): number {
  if (!Number.isFinite(level)) return 0;
  return Math.sqrt(level < 0 ? 0 : level > 1 ? 1 : level);
}

/** Map a smoothed 0–1 level onto a bar height in CSS px: 3 at 0, 20 at 1. */
export function barHeight(smoothed: number): number {
  return VU_BAR_MIN_HEIGHT + smoothed * (VU_BAR_MAX_HEIGHT - VU_BAR_MIN_HEIGHT);
}

/**
 * The rolling level history and the filter over it.
 *
 * Two arrays, deliberately: `targets` scrolls, `displayed` does not. Each bar
 * position runs its own moving average towards whatever target has scrolled
 * into it, which is what gives the band its trailing, liquid feel rather than
 * the hard step of a plain shift register. Reproduced from the reference app,
 * where the same split is `levelBuffer` (shifted) and `displayedBarLevels`
 * (not).
 *
 * `heights` is owned and mutated in place. At 25 Hz for the length of a
 * recording, returning a fresh array per frame would be pure garbage.
 */
export class LevelMeter {
  /** Scrolling window of curved levels; index 0 is oldest, last is newest. */
  private readonly targets = new Float64Array(VU_BAR_COUNT);

  /** Per-bar filter state, indexed by bar position rather than by sample. */
  private readonly displayed = new Float64Array(VU_BAR_COUNT);

  /**
   * Current bar heights in CSS px, left to right. Read-only by convention:
   * the caller renders it, `push` and `reset` are the only writers.
   */
  readonly heights = new Float64Array(VU_BAR_COUNT).fill(VU_BAR_MIN_HEIGHT);

  /** Accept one level sample and advance every bar one step. */
  push(level: number): void {
    const { targets, displayed, heights } = this;
    const last = VU_BAR_COUNT - 1;

    // Scroll one slot left, newest on the right.
    targets.copyWithin(0, 1);
    targets[last] = curve(level);

    for (let i = 0; i < VU_BAR_COUNT; i += 1) {
      // `noUncheckedIndexedAccess` types TypedArray reads as possibly
      // undefined; `i` is bounded by the array's own length, so it is not.
      const smoothed =
        (displayed[i] as number) * VU_SMOOTHING + (targets[i] as number) * (1 - VU_SMOOTHING);
      displayed[i] = smoothed;
      heights[i] = barHeight(smoothed);
    }
  }

  /** Return to silence: history cleared, filter cleared, all bars at 3px. */
  reset(): void {
    this.targets.fill(0);
    this.displayed.fill(0);
    this.heights.fill(VU_BAR_MIN_HEIGHT);
  }
}
