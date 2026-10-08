//! The Tauri realisation of [`wl_shell::host::Host`]: event broadcast, the
//! login item, the dock/taskbar presence, and the tray slot.

use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Emitter};
use tauri_plugin_autostart::ManagerExt;
use wl_shell::host::{Host, TrayControl};

use crate::tray::Tray;

pub struct TauriHost {
    app: AppHandle,
    tray: OnceLock<Arc<Tray>>,
}

impl TauriHost {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            tray: OnceLock::new(),
        }
    }

    /// Install the tray. Called once during setup, after `AppState` exists,
    /// because the tray reads settings and devices through it.
    pub fn set_tray(&self, tray: Arc<Tray>) {
        assert!(self.tray.set(tray).is_ok(), "the tray is installed once");
    }

    /// The concrete tray, for the Tauri-only menu handler: it reads the cached
    /// transcript and device list, which the shell-neutral [`TrayControl`]
    /// does not expose.
    pub fn tray_handle(&self) -> Option<&Arc<Tray>> {
        self.tray.get()
    }
}

impl Host for TauriHost {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Err(e) = self.app.emit(event, payload) {
            tracing::warn!(error = %e, event, "could not broadcast an event");
        }
    }

    fn launch_at_login_enabled(&self) -> Result<bool, String> {
        self.app
            .autolaunch()
            .is_enabled()
            .map_err(|e| e.to_string())
    }

    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        let manager = self.app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|e| e.to_string())
    }

    fn set_show_in_dock(&self, show: bool) {
        #[cfg(target_os = "macos")]
        {
            let policy = if show {
                tauri::ActivationPolicy::Regular
            } else {
                tauri::ActivationPolicy::Accessory
            };
            if let Err(e) = self.app.set_activation_policy(policy) {
                tracing::error!(error = %e, show, "could not change the activation policy");
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            use tauri::Manager;
            for label in crate::windows::MANAGED_LABELS {
                if let Some(window) = self.app.get_webview_window(label) {
                    if let Err(e) = window.set_skip_taskbar(!show) {
                        tracing::warn!(error = %e, label, "could not change the taskbar entry");
                    }
                }
            }
        }
    }

    fn tray(&self) -> Option<&dyn TrayControl> {
        self.tray_handle().map(|t| t.as_ref() as &dyn TrayControl)
    }
}
