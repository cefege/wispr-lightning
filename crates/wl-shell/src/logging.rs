//! Logging setup.
//!
//! Two sinks: a rolling file the user can attach to a bug report, and stderr
//! for development. Verbosity is a user setting, so the filter is reloadable
//! at runtime rather than fixed at startup.

use std::sync::OnceLock;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

type ReloadHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

static RELOAD: OnceLock<ReloadHandle> = OnceLock::new();
static GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Install the global subscriber. Safe to call more than once; later calls are
/// no-ops.
pub fn init() {
    if RELOAD.get().is_some() {
        return;
    }

    let log_path = wl_core::paths::log_file();
    let dir = log_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let _ = std::fs::create_dir_all(&dir);

    let file_appender = tracing_appender::rolling::daily(&dir, "WisprLightning.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = GUARD.set(guard);

    // RUST_LOG wins when set, so a developer can override without touching
    // the app's own setting.
    let base = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter, handle) = reload::Layer::new(base);
    let _ = RELOAD.set(handle);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false)
                .with_target(true),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .try_init();
}

/// Switch between normal and verbose logging without restarting.
///
/// Verbose logging includes request and response payload previews, which is
/// what makes a support report actionable — but it also puts transcript text
/// in the log, so it must stay opt-in.
pub fn set_verbose(verbose: bool) {
    let Some(handle) = RELOAD.get() else { return };
    let directive = if verbose {
        "debug,wl_core=trace,wl_providers=trace,wl_platform=trace"
    } else {
        "info"
    };
    if let Err(e) = handle.modify(|f| *f = EnvFilter::new(directive)) {
        tracing::warn!(error = %e, "could not change the log level");
    }
}
