//! A single UI Automation worker thread, shared by injection and app info.
//!
//! Two hard constraints shape this:
//!
//! * **Apartment.** `UIAutomation::new()` calls
//!   `CoInitializeEx(COINIT_MULTITHREADED)` on the calling thread, which fails
//!   with `RPC_E_CHANGED_MODE` on Tauri's STA main thread, and UIA proxies
//!   must not be shared across apartments. One dedicated MTA thread owns the
//!   `UIAutomation` object and every element derived from it; only plain data
//!   crosses back.
//! * **Latency.** Every UIA call is cross-process and can block for hundreds
//!   of milliseconds against a busy target. Requests therefore carry a
//!   deadline; on expiry the caller gives up while the worker keeps draining,
//!   so one wedged application costs one delayed answer rather than a wedged
//!   dictation.

use std::sync::LazyLock;
use std::time::Duration;

use crossbeam_channel::{bounded, Sender};
use uiautomation::UIAutomation;

type Job = Box<dyn FnOnce(&UIAutomation) + Send>;

struct Worker {
    jobs: Sender<Job>,
}

/// `None` once we know UI Automation is unavailable in this process, so the
/// failure is logged once rather than on every keystroke.
static WORKER: LazyLock<Option<Worker>> = LazyLock::new(|| {
    let (jobs, rx) = crossbeam_channel::unbounded::<Job>();
    let (ready_tx, ready_rx) = bounded::<bool>(1);

    let spawned = std::thread::Builder::new()
        .name("wl-uia".into())
        .spawn(move || {
            // Creating the automation object also joins this thread to the
            // MTA, which is exactly why it happens here.
            let automation = match UIAutomation::new() {
                Ok(automation) => {
                    let _ = ready_tx.send(true);
                    automation
                }
                Err(e) => {
                    tracing::warn!(error = %e, "UI Automation unavailable");
                    let _ = ready_tx.send(false);
                    return;
                }
            };
            while let Ok(job) = rx.recv() {
                job(&automation);
            }
        });

    if spawned.is_err() {
        tracing::warn!("could not spawn the UI Automation thread");
        return None;
    }
    // Bounded so a broken COM install cannot hang startup.
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(true) => Some(Worker { jobs }),
        _ => None,
    }
});

/// Run `f` on the UI Automation thread, giving up after `limit`.
///
/// `f` returns an `Option` so callers can express "asked, got nothing"
/// (unsupported pattern, password field) without inventing an error type; the
/// outer `None` additionally covers "UIA is unavailable or too slow".
pub(crate) fn with_uia<T, F>(what: &'static str, limit: Duration, f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce(&UIAutomation) -> Option<T> + Send + 'static,
{
    let worker = WORKER.as_ref()?;
    let (tx, rx) = bounded::<Option<T>>(1);
    let job: Job = Box::new(move |automation| {
        let _ = tx.send(f(automation));
    });
    if worker.jobs.send(job).is_err() {
        return None;
    }
    match rx.recv_timeout(limit) {
        Ok(value) => value,
        Err(_) => {
            tracing::debug!(what, ?limit, "UI Automation query timed out");
            None
        }
    }
}
