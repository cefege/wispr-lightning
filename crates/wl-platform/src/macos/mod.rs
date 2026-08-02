//! macOS platform integration.
//!
//! One module per capability, mirroring the service classes of the Swift
//! original so `docs/parity/platform-spec.md` can be checked file by file.
//!
//! Threading rule for everything here: every capability is driven from a
//! worker, so nothing may assume the main thread — and the two APIs that Apple
//! documents as main-thread-only go through [`main_thread::run`] rather than
//! being called where they are needed. Those two are Text Input Sources (the
//! Natural Mode layout map, `injector`) and `NSAppleScript` (pausing music,
//! `media`); see that module for the citations.
//!
//! Everything else here was audited against the macOS 26 SDK's Swift
//! concurrency annotations, which is where Apple now states main-thread
//! affinity: `NSPasteboard`, `NSWorkspace`, `NSRunningApplication` and
//! `NSNotificationCenter` carry no `@MainActor`, so the pasteboard snapshot,
//! the frontmost-app query and the sleep observer are legal from a worker.
//! `NSApplication` and `NSWindow` do carry it, and neither is touched from
//! this crate — the app layer owns them and hops for them.
//!
//! The Accessibility client API is the one loose end: Apple documents no
//! threading contract for it at all. See [`injector::ax`].
//!
//! The one genuinely cross-cutting concern is the **synthetic-input guard**
//! below. Every keystroke we synthesize is posted at `kCGHIDEventTap`, the
//! bottom of the event stack, so it travels through every session event tap in
//! the system — including the one the hotkey listener is built on. Both
//! `injector` and `hotkey` therefore need it, which is why it lives here.

mod appearance;
mod appinfo;
mod clipboard;
/// System-wide CoreAudio device-change listeners, consumed by
/// [`crate::audio_impl`] rather than exposed as a capability of its own.
pub(crate) mod devices;
pub mod hotkey;
mod injector;
mod lifecycle;
mod main_thread;
mod media;
mod ocr;
mod permissions;

pub use appearance::MacAppearance;
pub use appinfo::MacForegroundApp;
pub use clipboard::PasteboardSnapshot;
pub use hotkey::MacHotkeys;
pub use injector::MacInjector;
pub use lifecycle::MacLifecycle;
pub use media::MacMedia;
pub use ocr::MacScreenText;
pub use permissions::MacPermissions;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

/// Assemble the macOS capability set.
///
/// Infallible on purpose: every capability degrades at call time (OCR returns
/// nothing without Screen Recording, injection returns an error without
/// Accessibility) rather than refusing to construct, so the app can start,
/// show its settings window and walk the user through the grants.
pub fn platform() -> crate::Platform {
    crate::Platform {
        foreground: Arc::new(MacForegroundApp),
        injector: Arc::new(MacInjector::new()),
        screen: Arc::new(MacScreenText),
        media: Arc::new(MacMedia::new()),
        permissions: Arc::new(MacPermissions),
    }
}

/// Start global hotkey observation.
///
/// Kept out of [`platform`] because it owns a thread and therefore needs an
/// explicit lifetime. `Ok` does **not** mean hotkeys are working: without
/// Accessibility the event tap cannot exist yet, so surface
/// [`crate::hotkey::HotkeyBackend::is_healthy`] in the UI.
pub fn hotkeys() -> crate::Result<Arc<dyn crate::hotkey::HotkeyBackend>> {
    Ok(Arc::new(MacHotkeys::new()?))
}

pub fn lifecycle() -> Arc<dyn crate::Lifecycle> {
    Arc::new(MacLifecycle::new())
}

/// Start tracking the system accent colour.
///
/// Kept out of [`platform`] for the same reason as [`lifecycle`]: it owns a
/// notification registration, so it needs an explicit lifetime rather than
/// being reconstructed wherever it is wanted. Construct it on the main thread —
/// Tauri's `setup` — and the seed read costs nothing.
pub fn appearance() -> Arc<dyn crate::Appearance> {
    Arc::new(MacAppearance::new())
}

// ---------------------------------------------------------------------------
// The synthetic-input guard
// ---------------------------------------------------------------------------

/// Stamped into `kCGEventSourceUserData` on every event we synthesize — `"WLI!"`.
///
/// The macOS counterpart of the `dwExtraInfo` tag the Windows injector sets,
/// and the same value, so the two platforms are greppable as one mechanism.
/// `CGEventSourceSetUserData` attaches it to every event created from that
/// source, which is how a well-behaved event tap — ours or anyone else's —
/// tells our injection apart from the user's typing.
///
/// We cannot read it back ourselves: `handy-keys` decodes the `CGEvent` inside
/// its own tap callback and hands us a `KeyEvent` with no access to the event's
/// fields. [`begin_synthetic_input`] is what actually enforces the guard here;
/// see [`hotkey::accepts_key_event`].
pub(crate) const SYNTHETIC_USER_DATA: i64 = 0x574C_4921;

/// How long after synthesizing input the hotkey worker keeps ignoring events.
///
/// The events we post are re-injected at the bottom of the stack and reach the
/// tap a few milliseconds later; this window covers that lag with generous
/// slack, at the cost of swallowing a genuine hotkey press that lands
/// mid-injection. Swallowing it is the desirable failure: starting a recording
/// while we are still typing the previous one is worse. Identical to the
/// Windows constant, deliberately.
pub const SYNTHETIC_GUARD: Duration = Duration::from_millis(150);

/// Monotonic origin for the guard's timestamps. An `Instant` cannot live in an
/// atomic, so the guard stores milliseconds since this point.
static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

static SYNTHETIC_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// Declare that we are about to synthesize keyboard input for `window`.
///
/// Extends rather than replaces the current window, so overlapping bursts
/// (Natural Mode is one burst per character) never shorten it.
///
/// Public because `examples/probe.rs` arms the window from outside the crate to
/// exercise the HTK-011 parity row against a live event tap.
pub fn begin_synthetic_input(window: Duration) {
    let until = now_ms().saturating_add(window.as_millis() as u64);
    SYNTHETIC_UNTIL_MS.fetch_max(until, Ordering::Relaxed);
}

/// Whether keyboard input arriving right now is plausibly our own.
pub fn synthetic_input_in_flight() -> bool {
    now_ms() < SYNTHETIC_UNTIL_MS.load(Ordering::Relaxed)
}

fn now_ms() -> u64 {
    PROCESS_START.elapsed().as_millis() as u64
}
