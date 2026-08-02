/**
 * The overlay's pure state model.
 *
 * Kept free of the DOM so the width table and the label strings — the two
 * things parity is actually measured on — can be read, reviewed and driven by
 * the dev harness without a window.
 */

/**
 * Wire shape of the `overlay:state` event: the serde JSON of
 * `src-tauri/src/ui.rs`'s `OverlayState`, which is an externally tagged enum.
 * Unit variants arrive as bare strings, data variants as single-key objects.
 */
export type OverlayState =
  | "Hidden"
  | "Recording"
  | "Locked"
  | "Processing"
  | "Inserting"
  | { Retrying: { attempt: number; of: number } }
  | { Error: { message: string } }
  | { Recoverable: { message: string } };

/**
 * Wire shape of the `overlay:elapsed` event (`crate::ui::Elapsed`).
 *
 * `label` is already fully formatted by `wl_core::fsm::elapsed_label`: it is
 * `null` below 30 seconds, otherwise `M:SS` with `" \u{26A0}\u{FE0F}"`
 * appended once the warning level is above zero (OVL-032, OVL-033). The
 * overlay deliberately does not re-derive any of that — one formatter, one
 * place, and the Rust side already has unit tests on it.
 */
export interface Elapsed {
  label: string | null;
  /** Monotonic 0 -> 1 -> 2. Drives the background tint only (OVL-027/028). */
  warning: number;
}

/** The `data-state` attribute value; every visual rule keys off this. */
export type StateKey =
  | "hidden"
  | "recording"
  | "locked"
  | "processing"
  | "inserting"
  | "retrying"
  | "error"
  | "recoverable";

/**
 * Whether this state is a live recording.
 *
 * The two states that are, and only those two, show the VU strip, offer the
 * hover-revealed cancel ✕, and accept level samples. Named rather than
 * open-coded because all three of those rules have to agree: a strip that is
 * visible while the meter is deaf, or a ✕ that is clickable after the
 * microphone closed, are both bugs someone would have to reproduce live.
 */
export function isRecordingMode(key: StateKey): boolean {
  return key === "recording" || key === "locked";
}

/** What the user can do from the pill: recovery buttons, plus the ✕. */
export type OverlayAction = "retry" | "save" | "dismiss" | "cancel";

export interface View {
  key: StateKey;
  /** Main label text. Empty in Hidden, where nothing is painted. */
  label: string;
  /** Pill width in CSS px, before the elapsed-timer override. */
  width: number;
  /** Whether entering this state resets the warning level (OVL-034). */
  resetsWarning: boolean;
}

/**
 * Panel widths from v2-ui-spec §1.2 and §1.9. Height is 36 in every state.
 *
 * `recording` and `locked` are 130, not the 120 of the original spec: the pill
 * now carries the 88px VU strip, and 130 is what leaves it 5px of slack inside
 * the 16px edge insets.
 *
 * `recoverable` is 300, not 260. The Swift app had two retryable-error widths
 * depending on whether a save handler was supplied, but `OverlayState` carries
 * no such flag and `overlay_action` always offers `save`, so the port only
 * ever renders the three-button form. That collapses OVL-025 into OVL-026
 * deliberately rather than by omission.
 */
const WIDTH: Readonly<Record<StateKey, number>> = {
  hidden: 120,
  recording: 130,
  locked: 130,
  processing: 145,
  inserting: 145,
  retrying: 175,
  error: 180,
  recoverable: 300,
};

/** Width once the elapsed readout becomes visible at 30 s (OVL-032). */
export const ELAPSED_WIDTH = 200;

/** Height of the pill, identical in every state (OVL-017). */
export const PILL_HEIGHT = 36;

function view(key: StateKey, label: string): View {
  return {
    key,
    label,
    width: WIDTH[key],
    // Exactly the four states whose Swift entry points called
    // `warningState = 0`: show(), showLocked(), showProcessing(),
    // showInserting().
    resetsWarning: isRecordingMode(key) || key === "processing" || key === "inserting",
  };
}

/** Map a wire state onto everything the DOM needs to render it. */
export function describe(state: OverlayState): View {
  if (typeof state === "string") {
    switch (state) {
      case "Hidden":
        return view("hidden", "");
      // No label at all while recording: the VU band is the entire indicator,
      // and the reference app hides `mainLabel` in both states — "bars moving
      // = mic alive, bars jumping = voice detected".
      case "Recording":
        return view("recording", "");
      case "Locked":
        return view("locked", "");
      case "Processing":
        return view("processing", "Processing");
      // U+2026, not three periods.
      case "Inserting":
        return view("inserting", "Inserting\u2026");
    }
  }
  if ("Retrying" in state) {
    const { attempt, of } = state.Retrying;
    // U+2026, and the parenthesised N/M form, verbatim from OVL-023.
    return view("retrying", `Retrying\u2026 (${attempt}/${of})`);
  }
  if ("Error" in state) {
    return view("error", state.Error.message);
  }
  return view("recoverable", state.Recoverable.message);
}
