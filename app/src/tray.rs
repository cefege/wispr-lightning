//! The tray icon and its menu, realised from [`wl_shell::tray_model`].
//!
//! Everything here runs on the main thread: `tray-icon` handles are `!Send`,
//! so the core reaches the tray only through [`crate::bus::TrayCommand`]s,
//! which the root component drains into [`Tray::apply`].
//!
//! Covers MATRIX TRY-001 through TRY-021.

use std::str::FromStr;
use std::sync::Arc;

use dioxus::desktop::trayicon::menu::{
    accelerator::Accelerator, CheckMenuItem, IsMenuItem, Menu, MenuId, MenuItem,
    PredefinedMenuItem, Submenu,
};
use dioxus::desktop::trayicon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use wl_platform::audio::InputDevice;
use wl_shell::state::AppState;
use wl_shell::tray_model::{
    menu_model, MenuInput, MenuNode, ID_DEVICE_PREFIX, ID_LAST_TRANSCRIPTION, ID_NATURAL_MODE,
    ID_PAUSE, ID_QUIT, ID_SETTINGS, TRAY_ID,
};

use crate::bus::{Bus, TrayCommand};
use crate::windows::WindowName;

/// Deliberately not a template image (TRY-008): the two-colour mark is what
/// distinguishes idle from recording at a glance.
const ICON_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");
const ICON_RECORDING: &[u8] = include_bytes!("../icons/tray-recording.png");

fn decode_icon(png: &[u8]) -> anyhow::Result<Icon> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)?.into_rgba8();
    let (width, height) = image.dimensions();
    Ok(Icon::from_rgba(image.into_raw(), width, height)?)
}

fn build_items(nodes: &[MenuNode]) -> anyhow::Result<Vec<Box<dyn IsMenuItem>>> {
    nodes
        .iter()
        .map(|node| -> anyhow::Result<Box<dyn IsMenuItem>> {
            Ok(match node {
                MenuNode::Item {
                    id,
                    label,
                    enabled,
                    accelerator,
                } => {
                    let accelerator = accelerator
                        .map(Accelerator::from_str)
                        .transpose()
                        .map_err(|e| anyhow::anyhow!("bad accelerator: {e}"))?;
                    Box::new(MenuItem::with_id(
                        MenuId::new(id),
                        label,
                        *enabled,
                        accelerator,
                    ))
                }
                MenuNode::Check { id, label, checked } => Box::new(CheckMenuItem::with_id(
                    MenuId::new(id),
                    label,
                    true,
                    *checked,
                    None,
                )),
                MenuNode::Separator => Box::new(PredefinedMenuItem::separator()),
                MenuNode::Submenu { label, children } => {
                    let built = build_items(children)?;
                    let refs: Vec<&dyn IsMenuItem> = built.iter().map(Box::as_ref).collect();
                    Box::new(Submenu::with_items(label, true, &refs)?)
                }
            })
        })
        .collect()
}

fn build_menu(nodes: &[MenuNode]) -> anyhow::Result<Menu> {
    let built = build_items(nodes)?;
    let refs: Vec<&dyn IsMenuItem> = built.iter().map(Box::as_ref).collect();
    Ok(Menu::with_items(&refs)?)
}

pub struct Tray {
    state: Arc<AppState>,
    icon: TrayIcon,
    idle: Icon,
    recording_icon: Icon,
    /// Full text, not the preview: clicking the item copies all of it.
    last_transcription: Option<String>,
    devices: Vec<InputDevice>,
    recording: bool,
}

impl Tray {
    /// Create the tray icon with its menu. Main thread only.
    pub fn create(state: Arc<AppState>) -> anyhow::Result<Self> {
        let devices = state.audio.list_devices().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not enumerate input devices for the tray");
            Vec::new()
        });
        let idle = decode_icon(ICON_IDLE)?;
        let recording_icon = decode_icon(ICON_RECORDING)?;
        let menu = build_menu(&model(&state, None, &devices))?;
        let icon = TrayIconBuilder::new()
            .with_id(TRAY_ID)
            .with_icon(idle.clone())
            .with_icon_as_template(false)
            .with_tooltip("Wispr Lightning")
            .with_menu(Box::new(menu))
            // A macOS status item opens its menu on either button. On Windows
            // left-click is the icon's primary action (open Settings) and only
            // right-click opens the menu.
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .build()?;
        Ok(Self {
            state,
            icon,
            idle,
            recording_icon,
            last_transcription: None,
            devices,
            recording: false,
        })
    }

    pub fn apply(&mut self, command: TrayCommand) {
        match command {
            // TRY-002..004. Skipped when unchanged: called on every state
            // transition.
            TrayCommand::Recording(recording) => {
                if self.recording == recording {
                    return;
                }
                self.recording = recording;
                let icon = if recording {
                    &self.recording_icon
                } else {
                    &self.idle
                };
                if let Err(e) = self.icon.set_icon(Some(icon.clone())) {
                    tracing::warn!(error = %e, "could not change the tray icon");
                }
            }
            // TRY-011.
            TrayCommand::LastTranscription(text) => {
                self.last_transcription = Some(text);
                self.refresh();
            }
            // TRY-006: follows an unplug, even mid-recording.
            TrayCommand::Devices(devices) => {
                self.devices = devices;
                self.refresh();
            }
            TrayCommand::Refresh => self.refresh(),
        }
    }

    fn refresh(&self) {
        let nodes = model(
            &self.state,
            self.last_transcription.as_deref(),
            &self.devices,
        );
        match build_menu(&nodes) {
            Ok(menu) => self.icon.set_menu(Some(Box::new(menu))),
            Err(e) => tracing::warn!(error = %e, "could not rebuild the tray menu"),
        }
    }

    /// The notification-area icon's primary action. Windows only: macOS opens
    /// the menu on either button. Only `Up`, so dragging off cancels.
    pub fn on_icon_event(&self, bus: &Bus, event: &TrayIconEvent) {
        if cfg!(target_os = "macos") {
            return;
        }
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            bus.open_window(WindowName::Settings);
        }
    }

    pub fn on_menu_event(&self, bus: &Bus, id: &str) {
        if id == ID_QUIT {
            // TRY-020.
            tracing::info!("quit requested from the tray");
            tracing::info!("Wispr Lightning: shutting down");
            wl_shell::logging::flush();
            std::process::exit(0);
        }

        if id == ID_SETTINGS {
            bus.open_window(WindowName::Settings);
            return;
        }

        if id == ID_LAST_TRANSCRIPTION {
            // TRY-011: copy the whole transcript, not the elided preview.
            if let Some(text) = self.last_transcription.as_ref().filter(|t| !t.is_empty()) {
                let copied = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.clone()));
                if let Err(e) = copied {
                    tracing::warn!(error = %e, "could not copy the last dictation");
                }
            }
            return;
        }

        // Everything below writes settings through the single writer, so an
        // open settings window sees the same change (TRY-015 / TRY-017).
        let mut settings = self.state.settings();
        if let Some(device_id) = id.strip_prefix(ID_DEVICE_PREFIX) {
            if device_id.is_empty() {
                settings.mic_device_id = None;
                settings.mic_device_name = None;
            } else {
                let name = self
                    .devices
                    .iter()
                    .find(|d| d.id == device_id)
                    .map_or_else(|| device_id.to_string(), |d| d.name.clone());
                settings.mic_device_id = Some(device_id.to_string());
                settings.mic_device_name = Some(name);
            }
        } else if id == ID_PAUSE {
            settings.hotkey_paused = !settings.hotkey_paused;
        } else if id == ID_NATURAL_MODE {
            settings.natural_mode_enabled = !settings.natural_mode_enabled;
        } else {
            tracing::debug!(id, "unhandled tray menu item");
            return;
        }

        if let Err(e) = self.state.save_settings(settings) {
            tracing::error!(error = %e, id, "could not apply a tray menu change");
            // The rejected change left the old value in force; put the old
            // check mark back rather than leaving the menu lying.
            self.refresh();
        }
    }
}

fn model(state: &AppState, last: Option<&str>, devices: &[InputDevice]) -> Vec<MenuNode> {
    let settings = state.settings();
    menu_model(&MenuInput {
        last_transcription: last,
        devices,
        mic_device_id: settings.mic_device_id.as_deref(),
        hotkey_paused: settings.hotkey_paused,
        natural_mode: settings.natural_mode_enabled,
    })
}
