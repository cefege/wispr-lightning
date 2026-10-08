//! The menu-bar / system-tray icon and its menu.
//!
//! The menu is built in two steps. [`menu_model`] turns the current settings
//! and device list into a plain description — order, labels, enabled and
//! checked flags — with no Tauri types involved, and [`build_items`] realises
//! that description as real menu items. The split exists because the menu is
//! where two control surfaces meet (MATRIX TRY-015/017): which item carries the
//! check mark for a given settings state is behaviour worth testing, and it
//! cannot be tested through `muda`, which needs a main thread and a display
//! server.
//!
//! Covers MATRIX TRY-001 through TRY-021.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

use wl_platform::audio::InputDevice;

use wl_shell::host::TrayControl;
use wl_shell::state::AppState;
use wl_shell::tray_model::{
    menu_model, MenuInput, MenuNode, ID_DEVICE_PREFIX, ID_LAST_TRANSCRIPTION, ID_NATURAL_MODE,
    ID_PAUSE, ID_QUIT, ID_SETTINGS, TRAY_ID,
};

use crate::host::TauriHost;

// ---------------------------------------------------------------------------
// Realising the model
// ---------------------------------------------------------------------------

fn build_items<R: Runtime, M: Manager<R>>(
    manager: &M,
    nodes: &[MenuNode],
) -> tauri::Result<Vec<Box<dyn IsMenuItem<R>>>> {
    nodes
        .iter()
        .map(|node| -> tauri::Result<Box<dyn IsMenuItem<R>>> {
            Ok(match node {
                MenuNode::Item {
                    id,
                    label,
                    enabled,
                    accelerator,
                } => Box::new(MenuItem::with_id(
                    manager,
                    id,
                    label,
                    *enabled,
                    accelerator.as_ref(),
                )?),
                MenuNode::Check { id, label, checked } => Box::new(CheckMenuItem::with_id(
                    manager,
                    id,
                    label,
                    true,
                    *checked,
                    None::<&str>,
                )?),
                MenuNode::Separator => Box::new(PredefinedMenuItem::separator(manager)?),
                MenuNode::Submenu { label, children } => {
                    let built = build_items(manager, children)?;
                    let refs: Vec<&dyn IsMenuItem<R>> = built.iter().map(Box::as_ref).collect();
                    Box::new(Submenu::with_items(manager, label, true, &refs)?)
                }
            })
        })
        .collect()
}

fn build_menu<R: Runtime, M: Manager<R>>(
    manager: &M,
    nodes: &[MenuNode],
) -> tauri::Result<Menu<R>> {
    let built = build_items(manager, nodes)?;
    let refs: Vec<&dyn IsMenuItem<R>> = built.iter().map(Box::as_ref).collect();
    Menu::with_items(manager, &refs)
}

// ---------------------------------------------------------------------------
// The live tray
// ---------------------------------------------------------------------------

/// Deliberately not a template image (TRY-008): the icon is the product's own
/// two-colour mark, and tinting it to the menu-bar appearance would lose the
/// only thing distinguishing idle from recording at a glance.
const ICON_IDLE: Image<'static> = tauri::include_image!("icons/tray-idle.png");
const ICON_RECORDING: Image<'static> = tauri::include_image!("icons/tray-recording.png");

pub struct Tray {
    icon: TrayIcon,
    /// Full text, not the preview: clicking the item copies all of it.
    last_transcription: RwLock<Option<String>>,
    devices: RwLock<Vec<InputDevice>>,
    recording: AtomicBool,
}

impl Tray {
    /// Create the tray icon and install its menu.
    pub fn create(app: &AppHandle, state: &Arc<AppState>) -> tauri::Result<Arc<Self>> {
        let devices = state.audio.list_devices().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not enumerate input devices for the tray");
            Vec::new()
        });

        let settings = state.settings();
        let menu = build_menu(
            app,
            &menu_model(&MenuInput {
                last_transcription: None,
                devices: &devices,
                mic_device_id: settings.mic_device_id.as_deref(),
                hotkey_paused: settings.hotkey_paused,
                natural_mode: settings.natural_mode_enabled,
            }),
        )?;

        let icon = TrayIconBuilder::with_id(TRAY_ID)
            .icon(ICON_IDLE)
            .icon_as_template(false)
            .tooltip("Wispr Lightning")
            .menu(&menu)
            // A macOS status item opens its menu on either button, so Tauri's
            // default of `true` is correct there. Windows is the opposite
            // convention: left-click performs the notification area icon's
            // primary action and only right-click opens the context menu. An
            // app that opens a menu on left-click reads as a Mac port.
            .show_menu_on_left_click(cfg!(target_os = "macos"))
            .on_tray_icon_event(on_tray_icon_event)
            .on_menu_event(on_menu_event)
            .build(app)?;

        Ok(Arc::new(Self {
            icon,
            last_transcription: RwLock::new(None),
            devices: RwLock::new(devices),
            recording: AtomicBool::new(false),
        }))
    }

    /// TRY-002 / TRY-003 / TRY-004.
    pub fn set_recording(&self, recording: bool) {
        // Reassigning the same image would still cross the main-thread
        // boundary, and this is called on every state transition.
        if self.recording.swap(recording, Ordering::AcqRel) == recording {
            return;
        }
        let icon = if recording { ICON_RECORDING } else { ICON_IDLE };
        if let Err(e) = self.icon.set_icon(Some(icon)) {
            tracing::warn!(error = %e, "could not change the tray icon");
        }
    }

    /// TRY-011: remember the whole transcript and rebuild the preview item.
    pub fn set_last_transcription(&self, text: &str) {
        *self.last_transcription.write() = Some(text.to_string());
        self.refresh();
    }

    /// TRY-006: adopt a new device list and rebuild, including while recording.
    pub fn set_devices(&self, devices: Vec<InputDevice>) {
        *self.devices.write() = devices;
        self.refresh();
    }

    /// Rebuild the menu from the current settings and cached device list.
    pub fn refresh(&self) {
        let app = self.icon.app_handle().clone();
        let Some(state) = app.try_state::<Arc<AppState>>() else {
            // Only reachable if the tray outlives managed state, i.e. during
            // shutdown. There is nothing left to rebuild for.
            return;
        };
        let settings = state.settings();

        let last = self.last_transcription.read();
        let devices = self.devices.read();
        let model = menu_model(&MenuInput {
            last_transcription: last.as_deref(),
            devices: &devices,
            mic_device_id: settings.mic_device_id.as_deref(),
            hotkey_paused: settings.hotkey_paused,
            natural_mode: settings.natural_mode_enabled,
        });

        match build_menu(&app, &model) {
            Ok(menu) => {
                if let Err(e) = self.icon.set_menu(Some(menu)) {
                    tracing::warn!(error = %e, "could not install the tray menu");
                }
            }
            Err(e) => tracing::warn!(error = %e, "could not rebuild the tray menu"),
        }
    }
}

impl TrayControl for Tray {
    fn refresh(&self) {
        Tray::refresh(self);
    }
    fn set_recording(&self, recording: bool) {
        Tray::set_recording(self, recording);
    }
    fn set_last_transcription(&self, text: &str) {
        Tray::set_last_transcription(self, text);
    }
    fn set_devices(&self, devices: Vec<InputDevice>) {
        Tray::set_devices(self, devices);
    }
}

/// The notification area icon's primary action, Windows only.
///
/// `show_menu_on_left_click` is false there, so a left click would otherwise do
/// nothing at all. Settings is the app's main window, and opening a main window
/// is what a left click on a Windows tray icon is expected to do.
///
/// Only `Up` is handled: acting on `Down` would fire before the user could drag
/// off the icon to cancel, which every other Windows tray app allows.
#[cfg(not(target_os = "macos"))]
fn on_tray_icon_event<R: Runtime>(icon: &TrayIcon<R>, event: tauri::tray::TrayIconEvent) {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};

    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        let app = icon.app_handle();
        if let Err(e) = crate::windows::open(app, crate::windows::WindowName::Settings) {
            tracing::error!(error = %e, "could not open the settings window from the tray");
        }
    }
}

/// macOS opens the menu on either button, so there is no separate click action
/// to take. Present only so the builder call site does not need a `cfg`.
#[cfg(target_os = "macos")]
fn on_tray_icon_event<R: Runtime>(_icon: &TrayIcon<R>, _event: tauri::tray::TrayIconEvent) {}

/// The installed tray. Only the Tauri menu handler needs the concrete type,
/// for the cached transcript and device list.
fn installed_tray<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<Tray>> {
    app.try_state::<Arc<TauriHost>>()?.tray_handle().cloned()
}

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref();

    if id == ID_QUIT {
        // TRY-020. Dropping managed state closes the stores and the database;
        // `exit` sends `code: Some(0)`, which the run loop deliberately does
        // not prevent.
        tracing::info!("quit requested from the tray");
        app.exit(0);
        return;
    }

    if id == ID_SETTINGS {
        if let Err(e) = crate::windows::open(app, crate::windows::WindowName::Settings) {
            tracing::error!(error = %e, "could not open the settings window");
        }
        return;
    }

    let Some(state) = app.try_state::<Arc<AppState>>() else {
        tracing::warn!(id, "tray menu event before the app state existed");
        return;
    };
    let state = state.inner().clone();
    let tray = installed_tray(app);

    if id == ID_LAST_TRANSCRIPTION {
        // TRY-011: copy the whole transcript, not the elided preview.
        let text = tray
            .as_ref()
            .and_then(|tray| tray.last_transcription.read().clone());
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            if let Err(e) = app.clipboard().write_text(text) {
                tracing::warn!(error = %e, "could not copy the last dictation");
            }
        }
        return;
    }

    // Everything below writes settings, and does it through the single writer
    // so the settings window sees the same change (TRY-015 / TRY-017).
    let mut settings = state.settings();

    if let Some(device_id) = id.strip_prefix(ID_DEVICE_PREFIX) {
        if device_id.is_empty() {
            settings.mic_device_id = None;
            settings.mic_device_name = None;
        } else {
            let name = tray
                .as_ref()
                .and_then(|tray| {
                    tray.devices
                        .read()
                        .iter()
                        .find(|d| d.id == device_id)
                        .map(|d| d.name.clone())
                })
                .unwrap_or_else(|| device_id.to_string());
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

    if let Err(e) = state.save_settings(settings) {
        tracing::error!(error = %e, id, "could not apply a tray menu change");
        // The rejected change left the old value in force, so put the old
        // check mark back rather than leaving the menu lying.
        if let Some(tray) = &tray {
            tray.refresh();
        }
    }
}
