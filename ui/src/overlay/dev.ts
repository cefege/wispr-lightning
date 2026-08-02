/**
 * Development-only driver for the overlay.
 *
 * Outside Tauri there are no `overlay:state` events, so `/overlay.html` in a
 * browser would sit blank forever and none of the seven states could be
 * eyeballed or measured. This module makes every one of them reachable from a
 * query string, from the number keys, and from `window.overlayDev` for
 * automated width checks.
 *
 * It is imported only from an `import.meta.env.DEV` branch in `main.ts`, so
 * Rollup drops the branch and this whole module from a production build. It
 * must never be imported unconditionally.
 */

import type { OverlayController } from "./overlay";
import type { Elapsed, OverlayState } from "./state";

/** Handle the browser harness and any automation hang their calls off. */
export interface OverlayDev {
  setState(state: OverlayState): void;
  setElapsed(elapsed: Elapsed): void;
  /** Measured pill box, for checking against the §1.9 width table. */
  measure(): { width: number; height: number };
  /** Push one 0–1 level, as `overlay:level` would. */
  setLevel(level: number): void;
  /**
   * Drive the strip from a sine sweep at the real 25 Hz for `seconds`, so the
   * scroll direction and the smoothing can be watched rather than inferred.
   * Returns a stop function.
   */
  sweep(seconds?: number): () => void;
}

declare global {
  interface Window {
    overlayDev?: OverlayDev;
  }
}

/** `?state=` values, in the order the number keys 1-8 cycle them. */
const PRESETS: ReadonlyArray<readonly [string, OverlayState]> = [
  ["hidden", "Hidden"],
  ["recording", "Recording"],
  ["locked", "Locked"],
  ["processing", "Processing"],
  ["inserting", "Inserting"],
  ["retrying", { Retrying: { attempt: 2, of: 3 } }],
  ["error", { Error: { message: "Connection failed — check your network" } }],
  ["recoverable", { Recoverable: { message: "Server error: 502 Bad Gateway" } }],
];

/** The backend's cadence: one 640-sample / 40 ms frame at 16 kHz. */
const LEVEL_INTERVAL_MS = 40;

export function installDevHarness(overlay: OverlayController): void {
  const params = new URLSearchParams(location.search);
  const pill = document.querySelector<HTMLElement>("#pill");

  overlay.onAction((action) => console.info("[overlay dev] action:", action));

  const apply = (name: string): void => {
    const preset = PRESETS.find(([key]) => key === name);
    if (preset === undefined) {
      console.warn(`[overlay dev] unknown state "${name}"; try one of`, PRESETS.map(([k]) => k));
      return;
    }
    let [, state] = preset;
    if (name === "retrying") {
      state = {
        Retrying: {
          attempt: Number(params.get("attempt") ?? 2),
          of: Number(params.get("of") ?? 3),
        },
      };
    } else if ((name === "error" || name === "recoverable") && params.has("message")) {
      const message = params.get("message") ?? "";
      state = name === "error" ? { Error: { message } } : { Recoverable: { message } };
    }
    overlay.setState(state);
  };

  apply(params.get("state") ?? "recording");

  // `?elapsed=545&warning=1` reproduces what wl_core::fsm::elapsed_label would
  // have sent, including the U+26A0 U+FE0F suffix, so the 200 px widening and
  // the warning tints can be seen without a real 9-minute recording.
  const seconds = params.get("elapsed");
  if (seconds !== null) {
    const total = Number(seconds);
    const warning = Number(params.get("warning") ?? 0);
    const base = `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
    overlay.setElapsed({
      label: total < 30 ? null : warning > 0 ? `${base} \u26A0\uFE0F` : base,
      warning,
    });
  }

  window.addEventListener("keydown", (e) => {
    const index = Number(e.key) - 1;
    const preset = PRESETS[index];
    if (preset !== undefined) apply(preset[0]);
  });

  const sweep = (seconds = 10): (() => void) => {
    const started = performance.now();
    const timer = window.setInterval(() => {
      const t = (performance.now() - started) / 1000;
      if (t > seconds) {
        window.clearInterval(timer);
        return;
      }
      // A 0.4 Hz swell with a 3 Hz ripple on top: slow enough to see the band
      // scroll right-to-left, fast enough that neighbouring bars differ.
      const swell = (1 - Math.cos(2 * Math.PI * 0.4 * t)) / 2;
      const ripple = (1 + Math.sin(2 * Math.PI * 3 * t)) / 2;
      overlay.setLevel(swell * (0.55 + 0.45 * ripple));
    }, LEVEL_INTERVAL_MS);
    return () => window.clearInterval(timer);
  };

  window.overlayDev = {
    setState: (state) => overlay.setState(state),
    setElapsed: (elapsed) => overlay.setElapsed(elapsed),
    setLevel: (level) => overlay.setLevel(level),
    sweep,
    measure: () => {
      const box = pill?.getBoundingClientRect();
      return { width: box?.width ?? 0, height: box?.height ?? 0 };
    },
  };

  if (params.has("sweep")) sweep(Number(params.get("sweep") ?? 10));

  console.info(
    "[overlay dev] press 1-8 to switch state, or use ?state=%s. `overlayDev.sweep()` drives the VU band.",
    PRESETS.map(([k]) => k).join("|"),
  );
}
