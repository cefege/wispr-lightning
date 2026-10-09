//! Build everything and wire it together, before the event loop starts.
//!
//! Fallible so a genuinely unusable environment (no database, no hotkey
//! backend) fails loudly at launch rather than looking alive and silently doing
//! nothing.

use std::sync::Arc;

use wl_core::db::{Database, DictionaryStore, HistoryStore, NotesStore};
use wl_core::settings::{LoadOutcome, Settings};
use wl_platform::Appearance;
use wl_providers::credentials::CredentialStore;
use wl_shell::ops::AccentColor;
use wl_shell::state::{AppState, AppStateParts};
use wl_shell::{logging, pipeline, spool};

use crate::bus::Bus;
use crate::host::DioxusHost;
use crate::ui_impl::DioxusUi;

pub struct App {
    pub state: Arc<AppState>,
    pub bus: Arc<Bus>,
    /// Kept alive for the life of the process: dropping it unregisters the
    /// accent-change notification.
    pub appearance: Arc<dyn Appearance>,
}

/// Must run inside the entered tokio runtime: `Pipeline::spawn` starts tasks.
pub fn build() -> anyhow::Result<App> {
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
    let sound = wl_platform::sound_impl::player(crate::paths::resource_dir());

    let credentials = Arc::new(CredentialStore::new());
    let provider = wl_providers::build(&settings);

    let spool_dir = wl_core::paths::pending_audio_dir();
    wl_core::paths::ensure_dir(&spool_dir)?;
    let spool = Arc::new(spool::Spool::new(spool_dir));

    // Read before any window exists, so the first webview gets it at once.
    let appearance = wl_platform::current::appearance();
    let bus = Arc::new(Bus::new(
        settings.clone(),
        appearance.accent().map(AccentColor::of),
    ));
    {
        let bus = Arc::clone(&bus);
        appearance.on_accent_change(Box::new(move |accent| {
            bus.accent.send_replace(Some(AccentColor::of(accent)));
        }));
    }

    let host = Arc::new(DioxusHost::new(Arc::clone(&bus), Arc::clone(&lifecycle)));
    let state = Arc::new(AppState::new(AppStateParts {
        host,
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

    // LIF-010 / TRY-009, and launch-at-login reasserted every launch: a login
    // item can be removed behind the app's back. The device, sound pack,
    // hotkey binding and microphone pre-warm are applied by `Pipeline::spawn`.
    state.apply_show_in_dock(settings.show_in_dock);
    state.apply_launch_at_login(settings.launch_at_login);

    let ui = Arc::new(DioxusUi::new(
        Arc::clone(&bus),
        Arc::clone(&audio),
        tokio::runtime::Handle::current(),
    ));
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
        downloads_dir: dirs::download_dir().unwrap_or_else(|| spool.dir().to_path_buf()),
        timings: pipeline::Timings::default(),
    };
    let pipeline = pipeline::Pipeline::spawn(deps);
    state.set_pipeline(Arc::clone(&pipeline));

    // LIF-011: sleeping mid-recording abandons the take rather than resuming
    // with a hole in it.
    {
        let pipeline = Arc::clone(&pipeline);
        lifecycle.on_sleep(Box::new(move || {
            tracing::info!("system is going to sleep; abandoning any recording");
            pipeline.abort();
        }));
    }

    check_permissions(&state);

    // Setup means the permissions the current configuration needs are usable
    // now; a revoked grant returns to the guided flow.
    let missing = wl_shell::ops::missing_required_permissions(
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
        // Consumed by the root component once the event loop runs.
        bus.open_main(None);
    }

    // LIF-013: a recording that was never transcribed is offered back.
    if let Some(recovered) = spool.recover_latest() {
        tracing::info!(path = %recovered.path.display(), packets = recovered.packets.len(),
            "recovered an unsent recording");
        pipeline.offer_recovery(recovered);
    }

    // History retention, off the launch path, logged rather than dropped.
    {
        let history = Arc::clone(&state.history);
        std::thread::spawn(move || match history.prune() {
            Ok(0) => {}
            Ok(deleted) => tracing::info!(deleted, "pruned history past its retention limits"),
            Err(err) => tracing::error!(%err, "could not prune history"),
        });
    }

    Ok(App {
        state,
        bus,
        appearance,
    })
}

/// Report every permission at launch. Prompting belongs to the first-run
/// window, which presents one native request at a time.
fn check_permissions(state: &AppState) {
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
