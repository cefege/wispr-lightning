/// <reference types="vite/client" />

/**
 * Overlay entry point.
 *
 * Two transports: Tauri events in the app, and a dev harness when the document
 * is opened in a plain browser. The harness lives behind `import.meta.env.DEV`
 * and behind a dynamic import, so a production build contains neither the
 * branch nor the module.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import "../app.css";
import "./overlay.css";
import { createOverlay } from "./overlay";
import type { Elapsed, OverlayState } from "./state";

const overlay = createOverlay(document);

// A right-click menu on a floating HUD is never wanted and, on macOS, opening
// one would put an activating window in front of the app being dictated into.
document.addEventListener("contextmenu", (e) => e.preventDefault());

if ("__TAURI_INTERNALS__" in window) {
  overlay.onAction((action) => {
    // Fire and forget. The shell owns what happens next — including hiding the
    // window — and there is nothing useful the pill could render on failure.
    void invoke("overlay_action", { action });
  });

  void listen<OverlayState>("overlay:state", (e) => overlay.setState(e.payload));
  void listen<Elapsed>("overlay:elapsed", (e) => overlay.setElapsed(e.payload));
  // ~25 Hz for the length of a recording, which is why it goes straight to the
  // controller: no queueing, no rAF coalescing. One frame per 40 ms is already
  // under a display refresh, and deferring would only add latency to the one
  // element whose whole job is to look live.
  void listen<number>("overlay:level", (e) => overlay.setLevel(e.payload));
} else if (import.meta.env.DEV) {
  void import("./dev").then((m) => m.installDevHarness(overlay));
}
