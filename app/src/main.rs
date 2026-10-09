//! Wispr Lightning, Dioxus desktop shell.
//!
//! One process: the tokio runtime the pipeline runs on, the tray, the overlay
//! (the root window), and the managed windows opened on demand.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod accent;
mod bus;
mod components;
mod host;
mod overlay;
mod paths;
mod platform;
mod root;
mod setup;
mod tray;
mod ui;
mod ui_impl;
mod window_state;
mod windows;

use std::sync::LazyLock;

use tokio::runtime::Runtime;

/// Shared by the pipeline and every Dioxus task: `launch` adopts the runtime
/// entered when it is called.
static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build the tokio runtime")
});

fn main() {
    wl_shell::logging::init();
    run();
    // Reached only on a failed or second launch: `launch` never returns, and
    // tray Quit flushes before it exits.
    wl_shell::logging::flush();
}

fn run() {
    // macOS opens the lock name as a file, so use a stable writable directory
    // rather than LaunchServices' unpredictable working directory. Windows
    // treats it as a named mutex; keep that name free of path separators.
    #[cfg(target_os = "macos")]
    let lock_name = {
        let app_support = wl_core::paths::app_support_dir();
        if let Err(e) = wl_core::paths::ensure_dir(&app_support) {
            tracing::error!(error = %e, path = %app_support.display(), "could not prepare application support directory");
            return;
        }
        app_support
            .join("com.wisprlightning.app")
            .to_string_lossy()
            .into_owned()
    };
    #[cfg(not(target_os = "macos"))]
    let lock_name = "com.wisprlightning.app".to_owned();

    // A second launch cannot reach the first instance's windows from another
    // process, so exiting is the whole of the behaviour.
    let instance = match single_instance::SingleInstance::new(&lock_name) {
        Ok(instance) => instance,
        Err(e) => {
            tracing::error!(error = %e, name = %lock_name, "could not check for another instance");
            return;
        }
    };
    if !instance.is_single() {
        tracing::info!("second instance launched; exiting");
        return;
    }

    let _enter = RUNTIME.enter();

    let app = match setup::build() {
        Ok(app) => app,
        Err(e) => {
            tracing::error!(error = %e, "Wispr Lightning could not start");
            return;
        }
    };

    dioxus::LaunchBuilder::desktop()
        .with_cfg(overlay::window_config())
        .with_context(app.state)
        .with_context(app.bus)
        .with_context(app.appearance)
        .launch(root::Root);
}
