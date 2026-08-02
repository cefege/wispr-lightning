//! Shared application state, and the one place user settings are written.
//!
//! Two control surfaces edit the same settings: the tray menu (input device,
//! pause, Natural Mode) and the settings window. In the Swift app each of them
//! mutated `AppSettings` and called `save()` itself, so every side effect —
//! rebinding the hotkey, flipping the activation policy, reloading the sound
//! pack — had to be duplicated at each call site or silently skipped at one of
//! them. [`AppState::save_settings`] is the single writer instead: both
//! surfaces hand it a whole [`Settings`] and it owns persistence, every side
//! effect, and the `settings:changed` broadcast. That is what MATRIX
//! TRY-015/SET-034 and TRY-017/SET-056 mean by "cannot drift".

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use tauri::{AppHandle, Emitter};
use tauri_plugin_autostart::ManagerExt;

use wl_core::db::{Database, DictionaryStore, HistoryStore, NotesStore};
use wl_core::settings::Settings;
use wl_platform::audio::AudioCapture;
use wl_platform::hotkey::HotkeyBackend;
use wl_platform::sound::SoundPlayer;
use wl_platform::Platform;
use wl_providers::credentials::CredentialStore;
use wl_providers::TranscriptionProvider;

use crate::spool::Spool;

/// Everything the IPC commands, the tray and the pipeline share.
///
/// Registered as Tauri managed state, so a command reaches it with
/// `tauri::State<'_, Arc<AppState>>`.
pub struct AppState {
    app: AppHandle,

    settings: Arc<RwLock<Settings>>,
    settings_path: PathBuf,

    pub db: Arc<Database>,
    pub history: Arc<HistoryStore>,
    pub dictionary: Arc<DictionaryStore>,
    pub notes: Arc<NotesStore>,

    pub platform: Platform,
    pub audio: Arc<dyn AudioCapture>,
    pub sound: Arc<dyn SoundPlayer>,
    pub hotkeys: Arc<dyn HotkeyBackend>,

    /// Rebuilt when a Deepgram request option changes.
    pub provider: Arc<RwLock<Arc<dyn TranscriptionProvider>>>,
    pub credentials: Arc<CredentialStore>,
    pub spool: Arc<Spool>,

    /// Set once during setup, after the state itself exists — the pipeline
    /// borrows the same handles, so it cannot be constructed any earlier.
    pipeline: OnceLock<Arc<crate::pipeline::Pipeline>>,
    tray: OnceLock<Arc<crate::tray::Tray>>,

    /// True between `hotkey_capture_begin` and `hotkey_capture_end`. Guards
    /// against a second capture discarding the chord the first one latched.
    capturing_hotkey: AtomicBool,
}

/// Handles needed to build an [`AppState`]. A struct rather than a 17-argument
/// constructor because every field is independently fallible during setup.
pub struct AppStateParts {
    pub app: AppHandle,
    pub settings: Settings,
    pub settings_path: PathBuf,
    pub db: Arc<Database>,
    pub history: Arc<HistoryStore>,
    pub dictionary: Arc<DictionaryStore>,
    pub notes: Arc<NotesStore>,
    pub platform: Platform,
    pub audio: Arc<dyn AudioCapture>,
    pub sound: Arc<dyn SoundPlayer>,
    pub hotkeys: Arc<dyn HotkeyBackend>,
    pub provider: Arc<dyn TranscriptionProvider>,
    pub credentials: Arc<CredentialStore>,
    pub spool: Arc<Spool>,
}

impl AppState {
    pub fn new(parts: AppStateParts) -> Self {
        Self {
            app: parts.app,
            settings: Arc::new(RwLock::new(parts.settings)),
            settings_path: parts.settings_path,
            db: parts.db,
            history: parts.history,
            dictionary: parts.dictionary,
            notes: parts.notes,
            platform: parts.platform,
            audio: parts.audio,
            sound: parts.sound,
            hotkeys: parts.hotkeys,
            provider: Arc::new(RwLock::new(parts.provider)),
            credentials: parts.credentials,
            spool: parts.spool,
            pipeline: OnceLock::new(),
            tray: OnceLock::new(),
            capturing_hotkey: AtomicBool::new(false),
        }
    }

    pub fn app(&self) -> &AppHandle {
        &self.app
    }

    /// A snapshot. Callers must not hold the settings lock across an `await`,
    /// and every one of them wants the whole struct anyway.
    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    /// The live handle, for the pipeline and anything else that must observe
    /// changes without being told about them.
    pub fn settings_handle(&self) -> Arc<RwLock<Settings>> {
        Arc::clone(&self.settings)
    }

    /// A second reference to the platform bundle. Every field is an `Arc`, so
    /// this shares the implementations rather than duplicating them.
    pub fn platform_handles(&self) -> Platform {
        Platform {
            foreground: Arc::clone(&self.platform.foreground),
            injector: Arc::clone(&self.platform.injector),
            screen: Arc::clone(&self.platform.screen),
            media: Arc::clone(&self.platform.media),
            permissions: Arc::clone(&self.platform.permissions),
        }
    }

    pub fn set_pipeline(&self, pipeline: Arc<crate::pipeline::Pipeline>) {
        // A second call would leave half the app talking to an orphaned
        // pipeline, so it is a programming error rather than a race.
        debug_assert!(self.pipeline.get().is_none(), "pipeline already installed");
        let _ = self.pipeline.set(pipeline);
    }

    pub fn pipeline(&self) -> Option<&Arc<crate::pipeline::Pipeline>> {
        self.pipeline.get()
    }

    pub fn set_tray(&self, tray: Arc<crate::tray::Tray>) {
        debug_assert!(self.tray.get().is_none(), "tray already installed");
        let _ = self.tray.set(tray);
    }

    pub fn tray(&self) -> Option<&Arc<crate::tray::Tray>> {
        self.tray.get()
    }

    pub fn is_capturing_hotkey(&self) -> bool {
        self.capturing_hotkey.load(Ordering::Acquire)
    }

    /// Arm hotkey capture. Idempotent: re-arming an already-armed capture is a
    /// no-op and never discards a chord the backend has already latched.
    ///
    /// The webview has no way to observe the press — that is the whole point
    /// of capturing in the backend — so the settings UI polls
    /// [`Self::end_hotkey_capture`]. Idempotence is what makes that poll
    /// race-free: re-arming between two polls cannot lose anything.
    pub fn begin_hotkey_capture(&self) {
        if self.capturing_hotkey.swap(true, Ordering::AcqRel) {
            return;
        }
        self.hotkeys.begin_capture();
        if let Some(pipeline) = self.pipeline.get() {
            pipeline.set_capturing_hotkey(true);
        }
    }

    /// Poll the capture. `Some` disarms it; `None` leaves it armed.
    ///
    /// Leaving it armed on `None` closes the window between a poll that finds
    /// nothing and the caller's next arm, during which a press would otherwise
    /// be silently dropped. `None` means "nothing usable yet", never "clear the
    /// binding" — the caller must leave the existing hotkey alone.
    ///
    /// There is deliberately no timeout here. A backend timer racing the UI's
    /// own give-up timer is how a capture ends up binding a key seconds after
    /// the user abandoned it; the UI cancels by simply not polling again.
    pub fn end_hotkey_capture(&self) -> Option<wl_core::settings::Hotkey> {
        if !self.capturing_hotkey.load(Ordering::Acquire) {
            return None;
        }
        // The backend's `end_capture` always disarms, so re-arm it whenever it
        // came back empty. An invalid chord can never fire and is therefore
        // indistinguishable from nothing having been pressed.
        let captured = match self.hotkeys.end_capture().filter(|hk| hk.is_valid()) {
            Some(hk) => hk,
            None => {
                self.hotkeys.begin_capture();
                return None;
            }
        };

        self.capturing_hotkey.store(false, Ordering::Release);
        if let Some(pipeline) = self.pipeline.get() {
            pipeline.set_capturing_hotkey(false);
        }
        Some(captured)
    }

    /// Abandon a capture without adopting anything.
    pub fn cancel_hotkey_capture(&self) {
        if !self.capturing_hotkey.swap(false, Ordering::AcqRel) {
            return;
        }
        let _ = self.hotkeys.end_capture();
        if let Some(pipeline) = self.pipeline.get() {
            pipeline.set_capturing_hotkey(false);
        }
    }

    /// Persist `next`, apply every side effect it implies, and tell the rest of
    /// the app.
    ///
    /// The disk write happens first: if it fails, the in-memory settings are
    /// left untouched, so what the app is doing always matches what is on disk.
    /// A side effect that fails is logged and the rest still run — a user who
    /// cannot be added to the login items should not also lose their new
    /// hotkey.
    pub fn save_settings(&self, next: Settings) -> Result<Settings, String> {
        let mut next = next;
        // Defensive: a settings blob round-tripped through the webview can
        // still carry legacy Carbon keycodes if the user hand-edited the file.
        next.migrate();

        let previous = {
            let current = self.settings.read();
            if *current == next {
                return Ok(next);
            }
            current.clone()
        };

        next.save(&self.settings_path)
            .map_err(|e| format!("could not write settings: {e}"))?;
        *self.settings.write() = next.clone();

        self.apply(&previous, &next);

        if let Err(e) = self.app.emit("settings:changed", &next) {
            tracing::warn!(error = %e, "could not broadcast settings:changed");
        }
        Ok(next)
    }

    /// Everything that must happen outside the settings file when a field
    /// changes.
    ///
    /// Only the effects Tauri owns live here. The hotkey binding, the pause
    /// state, the input device, the microphone pre-warm and the sound pack are
    /// [`crate::pipeline::Pipeline`]'s: it reads the same `Arc<RwLock<Settings>>`
    /// and re-applies them on `settings_changed`, debouncing the microphone so
    /// a settings window that saves on every keystroke does not thrash the
    /// audio device. Applying them here as well would give one decision two
    /// owners.
    ///
    /// Each block below is guarded on its own field: rewriting the login item
    /// on every unrelated toggle would be a filesystem write per keystroke.
    fn apply(&self, previous: &Settings, next: &Settings) {
        if previous.verbose_logging != next.verbose_logging {
            crate::logging::set_verbose(next.verbose_logging);
        }

        // Deepgram snapshots these request options when it is constructed.
        if previous.deepgram_model != next.deepgram_model
            || previous.deepgram_keyterm_boost != next.deepgram_keyterm_boost
            || previous.deepgram_language != next.deepgram_language
            || previous.command_mode_enabled != next.command_mode_enabled
        {
            self.reload_provider(next);
        }

        if previous.launch_at_login != next.launch_at_login {
            self.apply_launch_at_login(next.launch_at_login);
        }

        if previous.show_in_dock != next.show_in_dock {
            self.apply_show_in_dock(next.show_in_dock);
        }

        match self.pipeline.get() {
            Some(pipeline) => pipeline.settings_changed(),
            // Unreachable in the running app: settings can only be saved from
            // the tray or a window, both of which exist after setup completes.
            None => tracing::warn!("settings changed before the pipeline was running"),
        }

        // The tray shows the device check mark, the pause title and the
        // Natural Mode check, so it is rebuilt for any of those.
        if let Some(tray) = self.tray.get() {
            tray.refresh();
        }
    }

    /// Rebuild Deepgram from the current request settings.
    pub fn reload_provider(&self, settings: &Settings) {
        *self.provider.write() = wl_providers::build(settings);
    }

    /// LIF-015 / SET-075: launch-at-login failures must surface, because a
    /// Windows Run-key write can be blocked by policy or antivirus and a
    /// toggle that silently turns itself back off looks like a broken feature.
    pub fn apply_launch_at_login(&self, enabled: bool) {
        let manager = self.app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        if let Err(e) = result {
            tracing::error!(error = %e, enabled, "could not change launch at login");
            let _ = self.app.emit(
                "settings:error",
                format!("Could not change Launch at login: {e}"),
            );
        }
    }

    /// TRY-009 / SET-076. macOS switches the activation policy live; Windows
    /// has no Dock, so the analogue is the taskbar entry on the real windows.
    /// The overlay is deliberately excluded — it must stay out of the taskbar
    /// and out of Alt-Tab in every configuration.
    pub fn apply_show_in_dock(&self, show: bool) {
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
}
