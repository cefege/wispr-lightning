//! Wispr Lightning: application shell.
//!
//! This crate is the only place that knows about Tauri. It owns the tray, the
//! windows, the IPC commands and the pipeline that wires
//! [`wl_platform`] capabilities to [`wl_providers`] backends. All portable
//! logic lives in [`wl_core`].

pub mod commands;
pub mod logging;
pub mod overlay;
pub mod pipeline;
pub mod spool;
pub mod state;
pub mod tray;
pub mod ui;
pub mod windows;

use std::sync::Arc;

use tauri::Manager;

use wl_core::db::{Database, DictionaryStore, HistoryStore, NotesStore};
use wl_core::settings::{LoadOutcome, Settings};
use wl_providers::credentials::CredentialStore;

use crate::state::{AppState, AppStateParts};

/// Application entry point, shared by the desktop binary and mobile targets.
pub fn run() {
    logging::init();

    let builder = tauri::Builder::default()
        // Registered first so a second launch can focus an already-open
        // window without creating another tray process.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            tracing::info!(
                ?argv,
                ?cwd,
                "second instance launched; focusing existing app"
            );
            // Do not *create* a window here: a second launch of a tray app must
            // not conjure UI the user did not ask for. Raising one that is
            // already open is the whole of the expected behaviour.
            for label in windows::MANAGED_LABELS {
                if let Some(window) = app.get_webview_window(label) {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.set_focus();
                        break;
                    }
                }
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(autostart_plugin())
        // SET-007: the Swift settings window used an `NSWindow` frame autosave
        // name to survive a relaunch. The overlay is denied because its
        // geometry is computed from the monitor's work area on every show —
        // restoring a remembered position would drop it wherever it was on a
        // display that may no longer exist.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_denylist(&[overlay::OVERLAY_LABEL])
                .build(),
        );

    // Manages the panel registry `WebviewWindow::to_panel` writes into.
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .invoke_handler(tauri::generate_handler![
            commands::settings_get,
            commands::settings_save,
            commands::onboarding_complete,
            commands::audio_devices,
            commands::sound_preview,
            commands::sound_packs,
            commands::deepgram_status,
            commands::deepgram_health,
            commands::deepgram_key_save,
            commands::deepgram_key_clear,
            commands::history_list,
            commands::history_search,
            commands::history_delete,
            commands::history_clear,
            commands::dictionary_list,
            commands::dictionary_add,
            commands::dictionary_update,
            commands::dictionary_delete,
            commands::dictionary_import_csv,
            commands::notes_list,
            commands::notes_add,
            commands::notes_update,
            commands::notes_delete,
            commands::hotkey_capture_begin,
            commands::hotkey_capture_end,
            commands::hotkey_set_paused,
            commands::permissions_status,
            commands::permissions_request,
            commands::permissions_open_settings,
            commands::overlay_action,
            commands::window_open,
            commands::app_quit,
            commands::accent_color,
        ])
        .setup(|app| {
            // A menu-bar app has no windows at rest; without this it would show
            // a Dock icon during startup before `show_in_dock` is read.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            setup(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri application")
        .run(|_app, event| match event {
            // Closing the last window must not quit: this app lives in the
            // tray. Matching `code: None` prevents only implicit exits, so an
            // explicit Quit from the tray still works.
            tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } => api.prevent_exit(),
            // LIF-012 / TRY-020. Dropping managed state closes the stores and
            // the database handle; the log line is what makes a clean shutdown
            tauri::RunEvent::Exit => {
                tracing::info!("Wispr Lightning: shutting down");
            }
            _ => {}
        });
}

/// Launch-at-login, configured per platform.
///
/// `macos_launcher` only exists in the plugin's macOS build, so the selection
/// cannot be a runtime branch. LaunchAgent rather than AppleScript: the
/// AppleScript backend drives System Events and triggers a TCC automation
/// prompt the user has no reason to expect at install time. Windows has a
/// single backend, the `HKCU\…\Run` key.
fn autostart_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let builder = tauri_plugin_autostart::Builder::new();

    #[cfg(target_os = "macos")]
    let builder = builder.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);

    builder.build()
}

/// Build everything and wire it together.
///
/// Fallible so a genuinely unusable environment — no database, no hotkey
/// backend — fails loudly at launch rather than looking alive and silently
/// doing nothing, which is the failure mode the Swift version shipped with.
fn setup(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let settings_path = wl_core::paths::settings_file();
    let (settings, outcome) = Settings::load(&settings_path);
    match &outcome {
        LoadOutcome::Fresh => tracing::info!("no settings file yet; starting with defaults"),
        LoadOutcome::Loaded => {}
        LoadOutcome::MigratedHotkeys => tracing::info!("migrated legacy hotkey keycodes"),
        LoadOutcome::RestoredFromBackup => {
            tracing::warn!("settings.json was unusable; restored from the snapshot beside it")
        }
        LoadOutcome::Recovered { backup } => {
            tracing::error!(backup = %backup.display(), "settings were unreadable and have been reset")
        }
    }
    logging::set_verbose(settings.verbose_logging);

    let db = Arc::new(Database::open()?);
    let history = Arc::new(HistoryStore::new(Arc::clone(&db)));
    let dictionary = Arc::new(DictionaryStore::new(Arc::clone(&db)));
    let notes = Arc::new(NotesStore::new(Arc::clone(&db)));

    let platform = wl_platform::current::platform();
    let lifecycle = wl_platform::current::lifecycle();
    let hotkeys = wl_platform::current::hotkeys()?;
    let audio = wl_platform::audio_impl::capture()?;
    let sound = wl_platform::sound_impl::player(app.path().resource_dir()?.join("resources"));

    let credentials = Arc::new(CredentialStore::new());
    let provider = wl_providers::build(&settings);

    let spool_dir = wl_core::paths::pending_audio_dir();
    wl_core::paths::ensure_dir(&spool_dir)?;
    let spool = Arc::new(spool::Spool::new(spool_dir));

    let state = Arc::new(AppState::new(AppStateParts {
        app: app.clone(),
        settings: settings.clone(),
        settings_path,
        db,
        history: Arc::clone(&history),
        dictionary: Arc::clone(&dictionary),
        notes,
        platform,
        audio: Arc::clone(&audio),
        sound: Arc::clone(&sound),
        hotkeys: Arc::clone(&hotkeys),
        provider,
        credentials,
        spool: Arc::clone(&spool),
    }));
    // Managed before anything that might look it up: the tray's menu handler
    // and the overlay's `Ui` impl both reach it through `AppHandle`.
    app.manage(Arc::clone(&state));

    apply_startup_settings(&state, &settings);

    // Before any window exists, so the value is already cached when the first
    // webview asks for it. `setup` runs on the main thread, which is where the
    // macOS read wants to happen anyway.
    watch_accent(app);

    // LIF-006 / OVL-041: constructed but not shown, so the first hotkey press
    // pays no construction latency.
    let overlay = overlay::Overlay::create(app)?;
    let tray = tray::Tray::create(app, &state)?;
    state.set_tray(tray);

    // `Ui` is implemented for `Arc<Overlay>` rather than `Overlay`, because the
    // transient-error auto-dismiss outlives the call that scheduled it. Hence
    // the second `Arc` here: it is the trait object's own box, not a duplicate
    // of the overlay.
    let ui: Arc<dyn ui::Ui> = Arc::new(overlay);

    let deps = pipeline::PipelineDeps {
        settings: state.settings_handle(),
        platform: state.platform_handles(),
        audio,
        sound,
        hotkeys,
        provider: Arc::clone(&state.provider),
        history,
        dictionary,
        spool: Arc::clone(&spool),
        ui,
        downloads_dir: app
            .path()
            .download_dir()
            .unwrap_or_else(|_| spool.dir().to_path_buf()),
        timings: pipeline::Timings::default(),
    };
    // `spawn` starts tokio tasks, which needs an entered runtime context;
    // `setup` runs on the main thread outside one.
    let pipeline = tauri::async_runtime::block_on(async move { pipeline::Pipeline::spawn(deps) });
    state.set_pipeline(Arc::clone(&pipeline));

    // LIF-011: sleeping mid-recording must abandon the take rather than resume
    // with a hole in it.
    {
        let pipeline = Arc::clone(&pipeline);
        lifecycle.on_sleep(Box::new(move || {
            tracing::info!("system is going to sleep; abandoning any recording");
            pipeline.abort();
        }));
    }

    check_permissions(&state);

    // Setup is not a historical checkbox: it means the permissions required
    // by the current configuration are usable now. A re-signed build or a
    // revoked TCC grant must return to the same guided flow before the user
    // discovers the problem on their first dictation.
    let missing = commands::missing_required_permissions(
        &state.settings(),
        state.platform.permissions.as_ref(),
    );
    let mut current = state.settings();
    if !missing.is_empty() && current.did_complete_onboarding {
        tracing::warn!(permissions = %missing.join(", "), "setup required again");
        current.did_complete_onboarding = false;
        state
            .save_settings(current.clone())
            .map_err(anyhow::Error::msg)?;
    }
    if !current.did_complete_onboarding {
        windows::open(app, windows::WindowName::Settings)?;
    }

    // LIF-013: a recording that was never transcribed — because the app
    // crashed, or the network failed and the user quit — is offered back.
    if let Some(recovered) = spool.recover_latest() {
        tracing::info!(path = %recovered.path.display(), packets = recovered.packets.len(),
            "recovered an unsent recording");
        pipeline.offer_recovery(recovered);
    }

    // History retention, last and off the launch path. It is pure SQL against
    // an already-migrated database and nothing here waits on it, but the
    // result is logged rather than dropped: a wedged database silently
    // skipping this every launch is how the table grew unbounded before.
    {
        let history = Arc::clone(&state.history);
        std::thread::spawn(move || match history.prune() {
            Ok(0) => {}
            Ok(deleted) => tracing::info!(deleted, "pruned history past its retention limits"),
            Err(err) => tracing::error!(%err, "could not prune history"),
        });
    }

    Ok(())
}

/// Report every permission at launch.
///
/// Prompt orchestration belongs to the first-run window, which presents one
/// native request at a time and refuses to finish until it succeeds. Raising a
/// prompt here races the webview and can stack four unrelated system dialogs
/// before the user sees why the app is asking.
fn check_permissions(state: &Arc<AppState>) {
    use wl_platform::{Permission, PermissionState};

    for permission in [
        Permission::Microphone,
        Permission::Accessibility,
        Permission::InputMonitoring,
        Permission::ScreenRecording,
    ] {
        let status = state.platform.permissions.status(permission);
        match status {
            PermissionState::Granted | PermissionState::NotApplicable => {
                tracing::info!(?permission, ?status, "permission")
            }
            PermissionState::Denied | PermissionState::NotDetermined => {
                tracing::warn!(?permission, ?status, "permission is not granted")
            }
        }
    }
}

/// Apply the loaded settings' Tauri-side effects.
///
/// Only the two that Tauri owns. The device selection, sound pack, hotkey
/// binding and microphone pre-warm are applied by `Pipeline::spawn` from the
/// same settings handle — doing them here as well would open the microphone
/// twice on a `keepMicrophoneActive` launch, and would leave two owners for
/// one decision.
fn apply_startup_settings(state: &Arc<AppState>, settings: &Settings) {
    // LIF-010 / TRY-009.
    state.apply_show_in_dock(settings.show_in_dock);
    // Reasserted every launch: a login item can be removed behind the app's
    // back by a system migration or a cleanup tool, and the setting is what the
    // user actually asked for.
    state.apply_launch_at_login(settings.launch_at_login);
}

/// Read the system accent colour and keep re-broadcasting it.
///
/// The observer lives in managed state for two reasons: dropping it would
/// unregister the platform notification, and `accent_color` reads the colour it
/// caches. The webview cannot obtain this for itself — both engines answer the
/// CSS `AccentColor` keyword with a hardcoded blue — so the value has to travel
/// over IPC and land in a custom property.
fn watch_accent(app: &tauri::AppHandle) {
    let appearance = wl_platform::current::appearance();

    {
        let app = app.clone();
        appearance.on_accent_change(Box::new(move |accent| {
            commands::publish_accent(&app, accent);
        }));
    }

    app.manage(appearance);
}
