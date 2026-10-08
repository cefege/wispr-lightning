//! The side effects only a windowing shell can perform, behind a trait so the
//! shell-agnostic core never names a toolkit. The Tauri app implements [`Host`]
//! in `src-tauri`; the Dioxus app implements it in `app/`.

use wl_platform::audio::InputDevice;

/// Tray control surface as seen from the core: the tray model is rebuilt by
/// the shell, and the core only asks it to refresh or to adopt new state.
pub trait TrayControl: Send + Sync {
    /// Rebuild the menu from the current settings and cached device list.
    fn refresh(&self);
    /// Toggle the recording icon.
    fn set_recording(&self, recording: bool);
    /// Show the most recent transcript as the menu's preview item.
    fn set_last_transcription(&self, text: &str);
    /// Adopt a new device list and rebuild.
    fn set_devices(&self, devices: Vec<InputDevice>);
}

/// Everything the core needs from the windowing shell.
pub trait Host: Send + Sync {
    /// Broadcast `event` with `payload` to every window.
    fn emit(&self, event: &str, payload: serde_json::Value);

    /// Whether the OS login item is currently registered.
    fn launch_at_login_enabled(&self) -> Result<bool, String>;

    /// Register or unregister the OS login item.
    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String>;

    /// macOS: switch the activation policy. Elsewhere: taskbar entries of the
    /// managed windows. The overlay is never affected.
    fn set_show_in_dock(&self, show: bool);

    /// The tray, once installed during setup.
    fn tray(&self) -> Option<&dyn TrayControl>;
}
