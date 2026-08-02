/**
 * DOM controller for the recording overlay.
 *
 * Deliberately framework-free and deliberately not importing `lib/ipc.ts`.
 * This document is a 36 px pill with eight states; a component runtime and the
 * settings store would be many times the weight of the thing they render, on
 * the one surface the user sees on every single dictation.
 *
 * The controller writes two attributes, two strings, a width and eighteen bar
 * heights. Everything else — which children are visible, which colour the
 * bars are, which tint the background carries — is a CSS rule keyed off
 * `data-state`, which is also why no reset step can forget to restore a
 * colour: leaving the state is the restore.
 */

import {
  describe,
  ELAPSED_WIDTH,
  isRecordingMode,
  type Elapsed,
  type OverlayAction,
  type OverlayState,
} from "./state";
import { LevelMeter, VU_BAR_COUNT, VU_BAR_MIN_HEIGHT } from "./vu";

export interface OverlayController {
  setState(state: OverlayState): void;
  setElapsed(elapsed: Elapsed): void;
  /** Feed one 0–1 normalized microphone level to the VU strip. */
  setLevel(level: number): void;
  /** Install the handler for Retry / Save / dismiss / cancel. */
  onAction(handler: (action: OverlayAction) => void): void;
}

function mustFind<E extends Element>(root: ParentNode, id: string): E {
  const el = root.querySelector<E>(`#${id}`);
  if (el === null) {
    // The markup is static and shipped alongside this module, so a miss means
    // the document was edited without the controller. Fail loudly at boot
    // rather than silently rendering a half-dead pill during a dictation.
    throw new Error(`overlay: #${id} is missing from overlay.html`);
  }
  return el;
}

export function createOverlay(root: ParentNode): OverlayController {
  const pill = mustFind<HTMLElement>(root, "pill");
  const label = mustFind<HTMLElement>(root, "label");
  const time = mustFind<HTMLElement>(root, "time");
  const retry = mustFind<HTMLButtonElement>(root, "retry");
  const save = mustFind<HTMLButtonElement>(root, "save");
  const dismiss = mustFind<HTMLButtonElement>(root, "dismiss");
  const cancel = mustFind<HTMLButtonElement>(root, "cancel");

  const bars = mustFind<HTMLElement>(root, "vu").querySelectorAll<HTMLElement>(".vu-bar");
  if (bars.length !== VU_BAR_COUNT) {
    throw new Error(`overlay: overlay.html has ${bars.length} VU bars, expected ${VU_BAR_COUNT}`);
  }

  let view = describe("Hidden");
  let warning = 0;
  let elapsedLabel = "";
  let handler: ((action: OverlayAction) => void) | null = null;

  const meter = new LevelMeter();
  /** Last height written to each bar, so an unchanged frame costs no style
      write. Silence is the common case and it is entirely unchanged frames. */
  const painted = new Float64Array(VU_BAR_COUNT).fill(VU_BAR_MIN_HEIGHT);

  function paintBars(): void {
    for (let i = 0; i < VU_BAR_COUNT; i += 1) {
      // Rounded to a tenth: below that the difference is sub-pixel on every
      // display this ships to, and writing it only invalidates layout.
      const height = Math.round((meter.heights[i] as number) * 10) / 10;
      if (height === painted[i]) continue;
      painted[i] = height;
      // `bars` is a static NodeList of exactly VU_BAR_COUNT elements, checked
      // at construction.
      (bars[i] as HTMLElement).style.height = `${height}px`;
    }
  }

  function resetBars(): void {
    meter.reset();
    paintBars();
  }

  function render(): void {
    // The elapsed readout only ever accompanied a live recording; gating on the
    // state stops a late tick from widening a Processing pill to 200 (OVL-032).
    const showElapsed = elapsedLabel !== "" && isRecordingMode(view.key);

    pill.dataset.state = view.key;
    pill.dataset.warning = String(warning);
    pill.style.width = `${showElapsed ? ELAPSED_WIDTH : view.width}px`;
    label.textContent = view.label;
    time.textContent = showElapsed ? elapsedLabel : "";
  }

  for (const [button, action] of [
    [retry, "retry"],
    [save, "save"],
    [dismiss, "dismiss"],
    [cancel, "cancel"],
  ] as const) {
    button.addEventListener("click", () => {
      handler?.(action);
      if (action === "save") {
        // Confirm in place and refuse a second write of the same audio
        // (OVL-036). Reset happens on the next show(), never here.
        save.textContent = "Saved";
        save.disabled = true;
      }
    });
    // A click must not move focus into this document even by accident: the
    // window is non-activating, but a focused button would still swallow the
    // caret ring and, on a misconfigured build, the keystrokes with it.
    button.addEventListener("mousedown", (e) => e.preventDefault());
  }

  render();

  return {
    setState(state) {
      const next = describe(state);
      const changed = next.key !== view.key;
      // Latching hands-free mid-recording is Recording -> Locked with the
      // microphone still open. The band must keep flowing and simply turn
      // green; flattening it there would read as a dropout at the exact
      // moment the user let go of the key. This is why the reference calls
      // `resetLevelBars()` from show(), hide(), showSpinner(), showRetrying()
      // and configureErrorState() — from every entry point except
      // `showLocked()`.
      const staysLive = changed && isRecordingMode(view.key) && isRecordingMode(next.key);
      view = next;

      if (next.resetsWarning) warning = 0;
      if (changed) {
        // show()/hide() both start a fresh presentation: the timer restarts
        // from hidden and Save is offerable again (OVL-019, OVL-038).
        elapsedLabel = "";
        save.textContent = "Save";
        save.disabled = false;
        // Otherwise every state entry returns the band to silence, so a new
        // recording never opens on the tail of the last one and a hidden pill
        // holds no history to flash on the next show.
        if (!staysLive) resetBars();
      }
      render();
    },

    setElapsed(elapsed) {
      elapsedLabel = elapsed.label ?? "";
      // Monotonic within a recording, exactly as the Swift warningState was;
      // only a state transition can step it back down (OVL-034).
      warning = Math.max(warning, elapsed.warning);
      render();
    },

    setLevel(level) {
      // A level that arrives outside a recording is dropped, not buffered:
      // the audio path and the state path are separate events, so a frame in
      // flight when the microphone closes can land after `hide()`. Buffering
      // it would repopulate the band on an invisible window and then show that
      // stale bar at the start of the next dictation.
      if (!isRecordingMode(view.key)) return;
      meter.push(level);
      paintBars();
    },

    onAction(next) {
      handler = next;
    },
  };
}
