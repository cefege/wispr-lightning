//! The Dioxus realisation of [`wl_shell::host::Host`].
//!
//! Nothing here touches a window directly: the core calls these from worker
//! threads, and every window operation has to happen on the main thread. Each
//! method publishes onto the [`Bus`] and the main-thread consumers act on it.

use std::sync::Arc;

use wl_core::settings::Settings;
use wl_platform::audio::InputDevice;
use wl_platform::Lifecycle;
use wl_shell::host::{Host, TrayControl};

use crate::bus::{Bus, TrayCommand};

pub struct DioxusHost {
    bus: Arc<Bus>,
    lifecycle: Arc<dyn Lifecycle>,
    tray: BusTray,
}

impl DioxusHost {
    pub fn new(bus: Arc<Bus>, lifecycle: Arc<dyn Lifecycle>) -> Self {
        Self {
            tray: BusTray(Arc::clone(&bus)),
            bus,
            lifecycle,
        }
    }
}

impl Host for DioxusHost {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        match event {
            "settings:changed" => match serde_json::from_value::<Settings>(payload) {
                Ok(settings) => {
                    self.bus.settings.send_replace(settings);
                }
                Err(e) => tracing::warn!(error = %e, "settings:changed payload is not Settings"),
            },
            "settings:error" => match payload {
                serde_json::Value::String(message) => {
                    // No receiver is not an error: no window is open to show it.
                    let _ = self.bus.settings_error.send(message);
                }
                other => tracing::warn!(?other, "settings:error payload is not a string"),
            },
            _ => match Bus::topic(event) {
                Some(topic) => {
                    let _ = self.bus.changed.send(topic);
                }
                None => tracing::warn!(event, "dropping an event nothing listens for"),
            },
        }
    }

    fn launch_at_login_enabled(&self) -> Result<bool, String> {
        Ok(self.lifecycle.launch_at_login())
    }

    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        self.lifecycle
            .set_launch_at_login(enabled)
            .map_err(|e| e.to_string())
    }

    fn set_show_in_dock(&self, show: bool) {
        // Applied by the root component on the main thread, and read by every
        // window created afterwards.
        self.bus.show_in_dock.send_replace(show);
    }

    fn tray(&self) -> Option<&dyn TrayControl> {
        Some(&self.tray)
    }
}

/// [`TrayControl`] that queues the work for the main thread, where the root
/// component owns the real tray icon.
struct BusTray(Arc<Bus>);

impl TrayControl for BusTray {
    fn refresh(&self) {
        self.0.tray(TrayCommand::Refresh);
    }

    fn set_recording(&self, recording: bool) {
        self.0.tray(TrayCommand::Recording(recording));
    }

    fn set_last_transcription(&self, text: &str) {
        self.0
            .tray(TrayCommand::LastTranscription(text.to_string()));
    }

    fn set_devices(&self, devices: Vec<InputDevice>) {
        self.0.tray(TrayCommand::Devices(devices));
    }
}
