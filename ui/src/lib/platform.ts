/**
 * Which OS the webview is running on, and the label vocabulary that follows
 * from it.
 *
 * Detection is from the user-agent rather than `@tauri-apps/plugin-os` because
 * this has to be a synchronous constant: `main.ts` sets `data-platform` on
 * `<html>` before the first paint so the CSS never renders macOS radii for a
 * frame on Windows. WKWebView always reports `Macintosh`; WebView2 always
 * reports `Windows NT`.
 */

export type Platform = "macos" | "windows" | "other";

function detect(): Platform {
  const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
  if (ua.includes("Windows")) return "windows";
  if (ua.includes("Macintosh") || ua.includes("Mac OS X")) return "macos";
  return "other";
}

export const platform: Platform = detect();
export const isWindows = platform === "windows";
export const isMac = platform === "macos";

/**
 * Tag the document so `app.css` can pick the platform's radii and type sizes.
 * Only Windows differs, so only Windows gets an attribute.
 */
export function applyPlatformAttribute(): void {
  if (isWindows) document.documentElement.setAttribute("data-platform", "windows");
}

/** The log path quoted in the Verbose logging description, per platform. */
export const logFilePath = isWindows
  ? "%LOCALAPPDATA%\\WisprLightning\\Logs\\WisprLightning.log"
  : "~/Library/Logs/WisprLightning.log";

/**
 * macOS calls it the Dock, Windows calls it the taskbar, and MATRIX SET-076
 * requires the Windows build to relabel rather than to say "Dock" on a machine
 * that has none.
 */
export const showInDockLabel = isWindows ? "Show in taskbar" : "Show in Dock";
