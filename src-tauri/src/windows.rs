//! The app's ordinary windows: settings, history, dictionary and notes.
//!
//! All four are created on demand and then kept forever. Closing one is
//! intercepted and turned into a hide (see [`install_close_guard`]), so the
//! webview is never destroyed: reopening is instant, the Svelte state survives,
//! and — the reason it actually matters — a tray-only app whose last window was
//! destroyed would otherwise trip Tauri's implicit-exit path.
//!
//! Covers MATRIX SET-001..SET-008 and WIN-001.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// The four windows this module owns. The overlay is deliberately absent: it
/// has entirely different rules and lives in [`crate::overlay`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowName {
    Settings,
    History,
    Dictionary,
    Notes,
}

/// Labels of every window created here, for bulk operations such as toggling
/// the Windows taskbar entry.
pub const MANAGED_LABELS: [&str; 4] = ["settings", "history", "dictionary", "notes"];

/// Geometry and chrome for one window.
struct Spec {
    label: &'static str,
    title: &'static str,
    /// Value of the `window` query parameter the single-page frontend routes
    /// on. `ui/` builds one document for all four views, so the route is in the
    /// URL rather than in the filename.
    route: &'static str,
    width: f64,
    height: f64,
    min_width: f64,
    min_height: f64,
}

impl WindowName {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "settings" => Some(Self::Settings),
            "history" => Some(Self::History),
            "dictionary" => Some(Self::Dictionary),
            "notes" => Some(Self::Notes),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        self.spec().label
    }

    fn spec(self) -> Spec {
        match self {
            // SET-001 / SET-002 / SET-004: the Swift content rect and minimum,
            // and the window title verbatim.
            Self::Settings => Spec {
                label: "settings",
                title: "Wispr Lightning Settings",
                route: "settings",
                width: 860.0,
                height: 580.0,
                min_width: 680.0,
                min_height: 460.0,
            },
            // WIN-001: in the Swift app these three were panes inside the
            // settings detail area, about 640 x 580. Opening them as their own
            // windows keeps that content size.
            Self::History => Spec {
                label: "history",
                title: "History",
                route: "history",
                width: 640.0,
                height: 580.0,
                min_width: 480.0,
                min_height: 400.0,
            },
            Self::Dictionary => Spec {
                label: "dictionary",
                title: "Dictionary",
                route: "dictionary",
                width: 640.0,
                height: 580.0,
                min_width: 480.0,
                min_height: 400.0,
            },
            Self::Notes => Spec {
                label: "notes",
                title: "Notes",
                route: "notes",
                width: 640.0,
                height: 580.0,
                min_width: 480.0,
                min_height: 400.0,
            },
        }
    }
}

/// Show `name`, creating it the first time and reusing it afterwards.
///
/// SET-006: only a freshly built window is centred. A window the user has
/// already moved keeps its position, which is what `SettingsWindow`'s frame
/// autosave achieved on macOS (SET-007).
pub fn open<R: Runtime>(app: &AppHandle<R>, name: WindowName) -> tauri::Result<()> {
    let spec = name.spec();

    if let Some(window) = app.get_webview_window(spec.label) {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
        activate_app(app);
        return Ok(());
    }

    let url = WebviewUrl::App(format!("index.html?window={}", spec.route).into());
    let mut builder = WebviewWindowBuilder::new(app, spec.label, url)
        .title(spec.title)
        .inner_size(spec.width, spec.height)
        .min_inner_size(spec.min_width, spec.min_height)
        .resizable(true)
        .center()
        .visible(true);

    // A tray-first app defaults to no Dock icon and no taskbar entry
    // (LIF-010). "Show in Dock" flips this at runtime for the real windows;
    // see `AppState::apply_show_in_dock`.
    #[cfg(not(target_os = "macos"))]
    {
        let show_in_dock = app
            .try_state::<std::sync::Arc<crate::state::AppState>>()
            .map(|state| state.settings().show_in_dock)
            .unwrap_or(false);
        builder = builder.skip_taskbar(!show_in_dock);
    }

    // macOS: the unified toolbar look of the Swift settings window (SET-005).
    // The frontend draws its own header bar in the space this frees up.
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Visible)
            .hidden_title(false);
    }

    let window = builder.build()?;
    install_close_guard(&window);
    activate_app(app);
    Ok(())
}

/// Bring the application forward so a newly-shown window is not buried behind
/// whatever the user was in (SET-009).
///
/// Needed because the app runs as an *accessory* by default: ordering a window
/// front does not activate an accessory app, and tao's `set_focus` only reaches
/// `activateIgnoringOtherApps:` when the window was already visible — which it
/// is not on the show-from-hidden path. Asking `NSApp` directly, on the main
/// thread, after the window is up, is what actually raises it.
#[cfg(target_os = "macos")]
fn activate_app<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = app.run_on_main_thread(|| {
        let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
            return;
        };
        let ns_app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        // `activate()` is macOS 14+ and honours the system's activation
        // arbitration; the deprecated call is the fallback for macOS 13, which
        // `tauri.conf.json` still supports.
        #[allow(deprecated)]
        ns_app.activateIgnoringOtherApps(true);
        ns_app.activate();
    }) {
        tracing::warn!(error = %e, "could not activate the application");
    }
}

/// No analogue: on Windows, showing a window already brings it to the front.
#[cfg(not(target_os = "macos"))]
fn activate_app<R: Runtime>(_app: &AppHandle<R>) {}

/// Turn a close into a hide.
///
/// SET-008: the Swift window controller kept its window alive across closes
/// (`isReleasedWhenClosed = false`). Destroying the webview instead would throw
/// away the frontend's state and force a full reload on the next open, which is
/// visible as a flash of empty UI.
pub fn install_close_guard<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Err(e) = handle.hide() {
                tracing::warn!(error = %e, label = handle.label(), "could not hide the window");
            }
        }
    });
}
