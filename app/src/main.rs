//! Wispr Lightning, Dioxus desktop shell.
//!
//! One process: the tokio runtime the pipeline runs on, the tray, the overlay
//! (the root window), and the managed windows opened on demand.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod accent;
mod bus;
mod host;
mod overlay;
mod paths;
mod platform;
mod root;
mod setup;
mod tray;
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
    // A second launch cannot reach the first instance's windows from another
    // process, so exiting is the whole of the behaviour.
    let instance = match single_instance::SingleInstance::new("com.wisprlightning.app") {
        Ok(instance) => instance,
        Err(e) => {
            tracing::error!(error = %e, "could not check for another instance");
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
