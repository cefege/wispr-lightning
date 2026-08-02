//! Windows implementation of the platform traits.
//!
//! Three cross-cutting concerns live here because several submodules need
//! them:
//!
//! * **The process-wide MTA.** Every WinRT call (OCR, SMTC) fails with
//!   `CO_E_NOTINITIALIZED` on a thread with no apartment, and Tauri owns the
//!   main thread as an STA for WebView2. `CoIncrementMTAUsage` gives the
//!   process an implicit MTA that uninitialised threads join, which is the
//!   only apartment strategy that does not fight the UI thread.
//! * **Bounded calls.** A wedged shell component must never stall the
//!   recording path, so the slow system APIs run on a throwaway thread with a
//!   deadline.
//! * **The synthetic-input guard.** Our own `SendInput` bursts are real
//!   keyboard input as far as the low-level hook is concerned, so pasting a
//!   transcript with Ctrl+V would otherwise retrigger a Ctrl push-to-talk
//!   binding. Events are stamped with [`SYNTHETIC_TAG`] in `dwExtraInfo`, and
//!   because `handy-keys` does not surface that field the hotkey pump also
//!   consults [`synthetic_input_in_flight`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

mod appearance;
mod appinfo;
mod classify;
mod clipboard;
/// System-wide WASAPI device-change notifications, consumed by
/// [`crate::audio_impl`] rather than exposed as a capability of its own.
pub(crate) mod devices;
mod hotkey;
mod injector;
mod keystrokes;
mod lifecycle;
mod matching;
mod media;
mod ocr;
mod permissions;
mod uia;

pub use appearance::WindowsAppearance;
pub use appinfo::WindowsForegroundApp;
pub use hotkey::WindowsHotkeys;
pub use injector::WindowsInjector;
pub use lifecycle::WindowsLifecycle;
pub use media::WindowsMedia;
pub use ocr::WindowsScreenText;
pub use permissions::WindowsPermissions;

/// Construct the implementation set for this target.
pub fn platform() -> crate::Platform {
    crate::Platform {
        foreground: Arc::new(WindowsForegroundApp::new()),
        injector: Arc::new(WindowsInjector::new()),
        screen: Arc::new(WindowsScreenText::new()),
        media: Arc::new(WindowsMedia::new()),
        permissions: Arc::new(WindowsPermissions::new()),
    }
}

/// Install the global keyboard hook and start delivering hotkey transitions.
pub fn hotkeys() -> crate::Result<Arc<dyn crate::hotkey::HotkeyBackend>> {
    Ok(Arc::new(WindowsHotkeys::start()?))
}

/// Sleep notifications and launch-at-login.
pub fn lifecycle() -> Arc<dyn crate::Lifecycle> {
    Arc::new(WindowsLifecycle::new())
}

/// Start tracking the system accent colour.
///
/// Kept out of [`platform`] for the same reason as [`lifecycle`]: it owns an
/// event registration, so it needs an explicit lifetime rather than being
/// reconstructed wherever it is wanted.
pub fn appearance() -> Arc<dyn crate::Appearance> {
    Arc::new(WindowsAppearance::new())
}

/// Stamped into `dwExtraInfo` on every event we synthesize — `"WLI!"`.
///
/// Any low-level hook we own can compare against this to tell our injection
/// apart from the user's typing. AutoHotkey-style tools use the same trick.
pub(crate) const SYNTHETIC_TAG: usize = 0x574C_4921;

/// How long after a `SendInput` batch the hotkey pump keeps ignoring input.
///
/// The events we post travel through the same low-level hook as real
/// keystrokes and arrive a few milliseconds later; this window covers that lag
/// with generous slack, at the cost of swallowing a genuine hotkey press that
/// lands mid-injection. Swallowing it is the desirable failure: starting a
/// recording while we are still typing the previous one is worse.
pub(crate) const SYNTHETIC_GUARD: Duration = Duration::from_millis(150);

/// Monotonic origin for the guard's timestamps. An `Instant` cannot live in an
/// atomic, so the guard stores milliseconds since this point.
static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

static SYNTHETIC_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// Declare that we are about to synthesize keyboard input for `window`.
///
/// Extends rather than replaces the current window, so overlapping bursts
/// (natural-mode typing is one batch per character) never shorten it.
pub(crate) fn begin_synthetic_input(window: Duration) {
    let until = now_ms().saturating_add(window.as_millis() as u64);
    SYNTHETIC_UNTIL_MS.fetch_max(until, Ordering::Relaxed);
}

/// Whether keyboard input arriving right now is plausibly our own.
pub(crate) fn synthetic_input_in_flight() -> bool {
    now_ms() < SYNTHETIC_UNTIL_MS.load(Ordering::Relaxed)
}

fn now_ms() -> u64 {
    PROCESS_START.elapsed().as_millis() as u64
}

/// Give the process an implicit multi-threaded apartment.
///
/// Idempotent, and the reference is never given back: `CoDecrementMTAUsage`
/// on the last cookie would tear down every live WinRT proxy with it. Safe to
/// call from the STA main thread — `CoIncrementMTAUsage` is a reference
/// count, not an apartment switch.
pub(crate) fn ensure_mta() {
    static MTA: LazyLock<()> = LazyLock::new(|| {
        // SAFETY: no preconditions. Dropping the returned cookie is a no-op
        // (it is `Copy`), which is exactly the "never released" we want.
        match unsafe { windows::Win32::System::Com::CoIncrementMTAUsage() } {
            Ok(_cookie) => tracing::debug!("implicit MTA established"),
            Err(e) => {
                tracing::warn!(error = %e, "CoIncrementMTAUsage failed; WinRT calls may fail")
            }
        }
    });
    LazyLock::force(&MTA);
}

/// Run `f` on a throwaway thread and give up after `limit`.
///
/// Used for the WinRT calls that have no timeout of their own. On expiry the
/// worker is abandoned rather than killed: there is no safe way to cancel a
/// blocked COM call, and leaking one thread beats blocking dictation.
pub(crate) fn bounded<T, F>(what: &'static str, limit: Duration, f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = crossbeam_channel::bounded(1);
    if let Err(e) = std::thread::Builder::new()
        .name(format!("wl-{what}"))
        .spawn(move || {
            let _ = tx.send(f());
        })
    {
        tracing::warn!(what, error = %e, "could not spawn worker");
        return None;
    }
    match rx.recv_timeout(limit) {
        Ok(value) => Some(value),
        Err(_) => {
            tracing::warn!(what, ?limit, "timed out; abandoning worker");
            None
        }
    }
}

/// Run `f` on a throwaway thread that is its own single-threaded apartment.
///
/// The shell publishes no marshalling information for its objects, so a shell
/// call made from a multi-threaded apartment cannot reach them and fails —
/// classically with `SE_ERR_ACCESSDENIED`, and only for the verbs that happen
/// to involve a COM object, which is what makes the failure look intermittent
/// (Microsoft KB 287087). [`ensure_mta`] puts every COM-uninitialised thread
/// in this process into the implicit MTA, tokio's workers included, so a shell
/// call cannot borrow its caller's apartment: it needs one of its own.
///
/// Bounded like [`bounded`], and for a sharper reason: this apartment pumps no
/// messages, so a shell extension that waits on one must not be able to wedge
/// the caller.
pub(crate) fn on_sta<T, F>(what: &'static str, limit: Duration, f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    bounded(what, limit, move || {
        let _apartment = Sta::enter();
        f()
    })
}

/// Joins the calling thread to a single-threaded apartment and leaves it again
/// on drop. Private to [`on_sta`], which only ever enters one on a thread it
/// just created, so there is never an existing apartment to conflict with.
struct Sta(bool);

impl Sta {
    fn enter() -> Self {
        use windows::Win32::System::Com::{
            CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        };
        // SAFETY: no preconditions, and this thread has not initialised COM.
        // `COINIT_DISABLE_OLE1DDE` is what the shell documentation asks for and
        // costs nothing: we have no OLE1 clients.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if hr.is_err() {
            tracing::warn!(?hr, "could not enter a single-threaded apartment");
        }
        Self(hr.is_ok())
    }
}

impl Drop for Sta {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful `CoInitializeEx` above, on the
            // same thread.
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}
