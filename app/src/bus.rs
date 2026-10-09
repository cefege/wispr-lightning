//! In-process replacement for the Tauri event system.
//!
//! The pipeline and `AppState` publish from worker threads; Dioxus components
//! and the tray consume on the main thread. Every channel is a `tokio::sync`
//! type so a component can `await` it from a `use_future`.
//!
//! `watch` for "latest value wins" streams (the 25 Hz level feed coalesces for
//! free), `broadcast` for notifications every open window must see, and
//! `mpsc` for work only the main thread may do (tray, window creation), whose
//! single receiver the root component takes once.

use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc, watch};

use wl_core::settings::Settings;
use wl_platform::audio::InputDevice;
use wl_shell::ops::AccentColor;
use wl_shell::ui::{Elapsed, OverlayState};

use crate::windows::WindowName;

/// The change topics `ops` and the pipeline publish. Unknown topics are
/// logged and dropped by [`crate::host::DioxusHost`].
pub const HISTORY_CHANGED: &str = "history:changed";
pub const DICTIONARY_CHANGED: &str = "dictionary:changed";
pub const DEVICES_CHANGED: &str = "devices:changed";

/// Main-thread tray work. `tray-icon` handles are `!Send` on every platform.
#[derive(Debug)]
pub enum TrayCommand {
    Recording(bool),
    LastTranscription(String),
    Devices(Vec<InputDevice>),
    Refresh,
}

pub struct Bus {
    /// `settings:changed`.
    pub settings: watch::Sender<Settings>,
    /// `settings:error`: a non-fatal settings side effect failed.
    pub settings_error: broadcast::Sender<String>,
    /// One of the `*_CHANGED` topics.
    pub changed: broadcast::Sender<&'static str>,
    /// The system accent colour, re-published on every OS change.
    pub accent: watch::Sender<Option<AccentColor>>,
    pub overlay_state: watch::Sender<OverlayState>,
    pub elapsed: watch::Sender<Elapsed>,
    /// macOS activation policy / Windows taskbar entries of managed windows.
    pub show_in_dock: watch::Sender<bool>,
    pub level: watch::Sender<f32>,
    pub tray: mpsc::UnboundedSender<TrayCommand>,
    pub open_window: mpsc::UnboundedSender<WindowName>,
    receivers: Mutex<Option<MainThreadReceivers>>,
}

/// The receiving ends of the main-thread queues, taken once by the root
/// component.
pub struct MainThreadReceivers {
    pub tray: mpsc::UnboundedReceiver<TrayCommand>,
    pub open_window: mpsc::UnboundedReceiver<WindowName>,
}

impl Bus {
    pub fn new(settings: Settings, accent: Option<AccentColor>) -> Self {
        let (tray, tray_rx) = mpsc::unbounded_channel();
        let (open_window, open_window_rx) = mpsc::unbounded_channel();
        Self {
            settings: watch::Sender::new(settings),
            settings_error: broadcast::Sender::new(16),
            changed: broadcast::Sender::new(16),
            accent: watch::Sender::new(accent),
            overlay_state: watch::Sender::new(OverlayState::Hidden),
            elapsed: watch::Sender::new(Elapsed::default()),
            level: watch::Sender::new(0.0),
            show_in_dock: watch::Sender::new(false),
            tray,
            open_window,
            receivers: Mutex::new(Some(MainThreadReceivers {
                tray: tray_rx,
                open_window: open_window_rx,
            })),
        }
    }

    /// The main-thread receivers. `None` after the first call: there is one
    /// root component and it owns them.
    pub fn take_receivers(&self) -> Option<MainThreadReceivers> {
        self.receivers.lock().take()
    }

    /// Queue tray work. A send only fails after the root component is gone,
    /// i.e. during shutdown, when there is no tray left to update.
    pub fn tray(&self, command: TrayCommand) {
        let _ = self.tray.send(command);
    }

    pub fn open_window(&self, name: WindowName) {
        let _ = self.open_window.send(name);
    }

    /// Map a topic string onto its static, or `None` for an unknown topic.
    /// Callers pass either a bare subject (`"devices"`) or a full event name.
    pub fn topic(topic: &str) -> Option<&'static str> {
        match topic.split(':').next().unwrap_or(topic) {
            "history" => Some(HISTORY_CHANGED),
            "dictionary" => Some(DICTIONARY_CHANGED),
            "devices" => Some(DEVICES_CHANGED),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_accept_bare_and_full_names() {
        assert_eq!(Bus::topic("devices"), Some(DEVICES_CHANGED));
        assert_eq!(Bus::topic("devices:changed"), Some(DEVICES_CHANGED));
        assert_eq!(Bus::topic("history:changed"), Some(HISTORY_CHANGED));
        assert_eq!(Bus::topic("dictionary"), Some(DICTIONARY_CHANGED));
        assert_eq!(Bus::topic("settings:changed"), None);
    }
}
