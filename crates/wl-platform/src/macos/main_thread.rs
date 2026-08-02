//! The one way this crate reaches the main thread.
//!
//! Two macOS APIs used here are documented as main-thread-only, and both are
//! naturally reached from a worker:
//!
//! * **Text Input Sources** (`TISCopyCurrentKeyboardLayoutInputSource`, used to
//!   build the Natural Mode layout map). `TextInputSources.h`: *"TextInputSources
//!   API is not thread safe. If you are a UI application, you must call
//!   TextInputSources API on the main thread."*
//! * **`NSAppleScript`** (used to pause and resume music). It is the single
//!   entry in the *Main Thread Only Classes* list of Apple's Threading
//!   Programming Guide, Thread Safety Summary: *"The following class must be
//!   used only from the main thread of an application: NSAppleScript."* The
//!   requirement is live rather than historical — since the December 2025
//!   XProtect update an `NSAppleScript` first created off the main thread hangs
//!   the process rather than failing.
//!
//! Routing both through one bounded helper is what makes the requirement
//! structural: the objects never exist outside the closure, so they cannot
//! escape onto the calling thread, and no call site has to remember a rule.

use std::time::Duration;

use objc2::MainThreadMarker;
use parking_lot::Mutex;

/// Run `f` on the main thread and wait up to `timeout` for its result.
///
/// `f` is handed a [`MainThreadMarker`], and the two main-thread-only wrappers
/// in this module tree demand one. That is the whole enforcement mechanism:
/// there is no way to reach `TISCopyCurrentKeyboardLayoutInputSource` or
/// `NSAppleScript` without a marker, and no way to obtain a marker off the
/// main thread.
///
/// `None` means the main run loop did not service the block in time. It does
/// **not** mean the block was cancelled — it stays queued and will still run —
/// so anything whose loss would matter must be recorded by `f` itself rather
/// than inferred from the return value.
///
/// The main queue runs one operation at a time and starts equal-priority
/// operations in submission order, so two calls from the same worker land in
/// the order they were made.
pub(super) fn run<T, F>(f: F, timeout: Duration) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce(MainThreadMarker) -> T + Send + 'static,
{
    if let Some(mtm) = MainThreadMarker::new() {
        return Some(f(mtm));
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    let once = Mutex::new(Some(f));
    let block = block2::RcBlock::new(move || {
        // SAFETY: `NSOperationQueue::mainQueue` runs its operations on the main
        // thread; that is the only thing this block is ever scheduled on.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        if let Some(f) = once.lock().take() {
            let _ = tx.send(f(mtm));
        }
    });
    // SAFETY: `addOperationWithBlock:` copies the block, so it stays alive for
    // as long as the queued operation needs it.
    unsafe {
        objc2_foundation::NSOperationQueue::mainQueue().addOperationWithBlock(&block);
    }
    rx.recv_timeout(timeout).ok()
}
