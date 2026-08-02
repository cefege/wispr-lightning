/**
 * Entry point for the main document.
 *
 * The platform attribute and the system accent are both applied before mounting
 * so the first paint already has the right radii, type scale and accent colour;
 * doing either in a component would flash macOS metrics on Windows, or a blue
 * button at a user who chose orange, for a frame.
 */

import { mount } from "svelte";
import App from "./App.svelte";
import { accentColor, onSystemAccent, type AccentColor } from "./lib/ipc";
import { applyPlatformAttribute } from "./lib/platform";
import "./app.css";

applyPlatformAttribute();

/**
 * How long mounting will wait for the accent before giving up on it.
 *
 * The backend caches the colour before it creates any window, so the answer is
 * a lock read and this budget is never spent in practice. It exists so that a
 * wedged IPC bridge degrades to the stylesheet's fallback accent instead of
 * holding the window blank.
 */
const ACCENT_BUDGET_MS = 250;

/**
 * Write the accent onto `<html>` as an inline style, which outranks both the
 * `:root` block in `app.css` and the dark-appearance one — so the same four
 * properties override whichever appearance is in force.
 */
function applyAccent(accent: AccentColor): void {
  const style = document.documentElement.style;
  style.setProperty("--accent", accent.accent);
  style.setProperty("--accent-text", accent.text);
  style.setProperty("--accent-darker", accent.darker);
  style.setProperty("--accent-lighter", accent.lighter);
}

// Applied whenever the answer arrives; the race below only bounds how long the
// first paint waits for it. `null` means the platform reported no accent, which
// is not a failure — the stylesheet's fallback is the answer in that case.
const firstAccent = accentColor().then(
  (accent) => {
    if (accent !== null) applyAccent(accent);
  },
  () => {
    /* No bridge, or the query failed. The fallback stands. */
  },
);

await Promise.race([
  firstAccent,
  new Promise<void>((resolve) => setTimeout(resolve, ACCENT_BUDGET_MS)),
]);

// Never unsubscribed: the accent tracks the OS for as long as this document is
// alive, and this document only stops being alive with its window.
onSystemAccent(applyAccent);

const route = new URLSearchParams(window.location.search).get("window") ?? "settings";

const target = document.getElementById("app");
if (target === null) {
  throw new Error("index.html is missing its #app mount point");
}

export default mount(App, { target, props: { route } });
