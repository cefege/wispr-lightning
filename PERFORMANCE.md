# Wispr Lightning vs. Wispr Flow — Performance Summary

**Measured:** March 18, 2026 — both apps idle on the same machine (macOS 15.3)

---

## At a Glance

| Metric | Wispr Lightning | Wispr Flow | Difference |
|---|---|---|---|
| **RAM (idle)** | 18 MB | ~560 MB | **31× less** |
| **CPU (idle)** | ~0% | ~21% | **∞ reduction** |
| **Processes** | 1 | 11 | **11× fewer** |
| **App size on disk** | 5.2 MB | 438 MB | **84× smaller** |
| **Binary size** | 2.1 MB | 56 KB shell + 182 MB JS/assets | — |

---

## RAM

Wispr Flow spawns 11 separate OS processes at launch — four Chromium renderers, a GPU compositor, a network service, an audio helper, a plugin/video capture helper, a crashpad crash reporter, a native Swift helper, and the main Electron shell. Together they consume approximately **560 MB of RAM while doing nothing**.

Wispr Lightning runs as a **single native process using 18 MB** — less than the crashpad reporter alone in Wispr Flow.

## CPU

Wispr Flow's Electron runtime and background renderers keep the CPU active even when the app is sitting idle. Measured at rest: **~21% CPU across all processes**.

Wispr Lightning measured **0% CPU at idle** — the OS parks the process entirely between interactions.

## App Size

Wispr Flow ships a full copy of the Chromium browser engine:

- `Electron Framework.framework` — 255 MB
- JS/HTML/asset bundle (`app.asar` + resources) — 182 MB
- Total: **438 MB**

Wispr Lightning ships a single compiled binary and a handful of audio assets:

- Native binary — 2.1 MB
- Resources (icon + sounds) — 3.1 MB
- Total: **5.2 MB**

## Real-World Impact

On a **MacBook M1 Air with 8 GB of RAM**, Wispr Flow's ~560 MB idle footprint represents **7% of total system memory consumed before you've done anything**. Under a typical heavy workload — browser with multiple tabs, a code editor, Slack, Figma — available RAM shrinks quickly. Wispr Flow, which needs memory headroom for its Chromium renderers to stay healthy, consistently crashes under these conditions after extended use.

Wispr Lightning's 18 MB footprint is negligible in any workload. On the same machine under the same conditions, it remains stable indefinitely — because there is nothing to crash.

---

## Why the Difference

Wispr Flow is built on **Electron** — a framework that bundles Chromium and Node.js so web apps can run as desktop apps. Every user interaction travels through a JavaScript engine, an IPC layer, and a Chromium rendering pipeline before reaching the OS.

Wispr Lightning is written in **native Swift** using macOS's AppKit and AVFoundation APIs directly. There is no browser engine, no JavaScript runtime, no IPC between processes. The OS executes the app code directly.

The result is that Wispr Lightning uses the resources of a lightweight menu-bar utility — because that is exactly what it is.
