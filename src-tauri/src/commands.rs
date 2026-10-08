//! The Tauri IPC surface.
//!
//! Every command is a one-line wrapper over [`wl_shell::ops`], which owns the
//! behaviour and is shared with every other shell. Only what needs a Tauri API
//! lives here: the accent colour lookup and broadcast, opening a window, and
//! quitting the app.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use wl_core::db::models::{DictionaryEntry, NoteEntry, TranscriptEntry};
use wl_core::db::CsvImport;
use wl_core::settings::{Hotkey, Settings};
use wl_shell::ops::{self, fail};
use wl_shell::state::AppState;

pub use wl_shell::ops::{
    AccentColor, DeepgramBalance, DeepgramHealth, DeepgramStatus, DictionaryInput, InputDeviceInfo,
};

/// Commands return `Result<T, String>`: the webview has no way to act on a
/// typed error, and a rendered message is what the UI displays either way.
type Result<T, E = String> = std::result::Result<T, E>;

fn state(app: &AppHandle) -> Result<Arc<AppState>> {
    app.try_state::<Arc<AppState>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| "the application is still starting up".to_string())
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn settings_get(state: State<'_, Arc<AppState>>) -> Result<Settings> {
    Ok(state.settings())
}

#[tauri::command]
pub async fn settings_save(
    state: State<'_, Arc<AppState>>,
    settings: Settings,
) -> Result<Settings> {
    state.save_settings(settings)
}

#[tauri::command]
pub async fn onboarding_complete(state: State<'_, Arc<AppState>>) -> Result<Settings> {
    ops::onboarding_complete(&state)
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn audio_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<InputDeviceInfo>> {
    ops::audio_devices(&state)
}

#[tauri::command]
pub async fn sound_preview(
    state: State<'_, Arc<AppState>>,
    pack: Option<String>,
    cue: String,
) -> Result<()> {
    ops::sound_preview(&state, pack, &cue)
}

#[tauri::command]
pub async fn sound_packs(state: State<'_, Arc<AppState>>) -> Result<Vec<String>> {
    ops::sound_packs(&state)
}

// ---------------------------------------------------------------------------
// Deepgram
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn deepgram_status(state: State<'_, Arc<AppState>>) -> Result<DeepgramStatus> {
    ops::deepgram_status(&state)
}

#[tauri::command]
pub async fn deepgram_health(state: State<'_, Arc<AppState>>) -> Result<DeepgramHealth> {
    ops::deepgram_health(&state).await
}

#[tauri::command]
pub async fn deepgram_balance(state: State<'_, Arc<AppState>>) -> Result<DeepgramBalance> {
    ops::deepgram_balance(&state).await
}

#[tauri::command]
pub async fn deepgram_key_save(state: State<'_, Arc<AppState>>, key: String) -> Result<()> {
    ops::deepgram_key_save(&state, &key)
}

#[tauri::command]
pub async fn deepgram_key_clear(state: State<'_, Arc<AppState>>) -> Result<()> {
    ops::deepgram_key_clear(&state)
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn history_list(
    state: State<'_, Arc<AppState>>,
    limit: i64,
    offset: i64,
) -> Result<Vec<TranscriptEntry>> {
    ops::history_list(&state, limit, offset)
}

#[tauri::command]
pub async fn history_search(
    state: State<'_, Arc<AppState>>,
    query: String,
) -> Result<Vec<TranscriptEntry>> {
    ops::history_search(&state, &query)
}

#[tauri::command]
pub async fn history_delete(state: State<'_, Arc<AppState>>, id: String) -> Result<()> {
    ops::history_delete(&state, &id)
}

#[tauri::command]
pub async fn history_clear(state: State<'_, Arc<AppState>>) -> Result<()> {
    ops::history_clear(&state)
}

// ---------------------------------------------------------------------------
// Dictionary
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn dictionary_list(
    state: State<'_, Arc<AppState>>,
    kind: String,
) -> Result<Vec<DictionaryEntry>> {
    ops::dictionary_list(&state, &kind)
}

#[tauri::command]
pub async fn dictionary_add(
    state: State<'_, Arc<AppState>>,
    entry: DictionaryInput,
) -> Result<DictionaryEntry> {
    ops::dictionary_add(&state, entry)
}

#[tauri::command]
pub async fn dictionary_update(
    state: State<'_, Arc<AppState>>,
    entry: DictionaryInput,
) -> Result<()> {
    ops::dictionary_update(&state, entry)
}

#[tauri::command]
pub async fn dictionary_delete(state: State<'_, Arc<AppState>>, id: String) -> Result<()> {
    ops::dictionary_delete(&state, &id)
}

#[tauri::command]
pub async fn dictionary_import_csv(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<CsvImport> {
    ops::dictionary_import_csv(&state, std::path::Path::new(&path))
}

// ---------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn notes_list(
    state: State<'_, Arc<AppState>>,
    query: Option<String>,
) -> Result<Vec<NoteEntry>> {
    ops::notes_list(&state, query)
}

#[tauri::command]
pub async fn notes_add(
    state: State<'_, Arc<AppState>>,
    title: String,
    content: String,
) -> Result<NoteEntry> {
    ops::notes_add(&state, &title, &content)
}

#[tauri::command]
pub async fn notes_update(
    state: State<'_, Arc<AppState>>,
    id: String,
    title: String,
    content: String,
) -> Result<()> {
    ops::notes_update(&state, &id, &title, &content)
}

#[tauri::command]
pub async fn notes_delete(state: State<'_, Arc<AppState>>, id: String) -> Result<()> {
    ops::notes_delete(&state, &id)
}

// ---------------------------------------------------------------------------
// Hotkeys
// ---------------------------------------------------------------------------

/// Arm hotkey capture, suppressing normal handling so binding a key cannot
/// start a recording (SET-028).
#[tauri::command]
pub async fn hotkey_capture_begin(state: State<'_, Arc<AppState>>) -> Result<()> {
    state.begin_hotkey_capture();
    Ok(())
}

/// Poll the capture.
///
/// `None` means "nothing usable pressed yet" and leaves the capture armed, so
/// the settings window can poll without a gap in which a press would be lost.
/// It must never be read as "clear the binding".
#[tauri::command]
pub async fn hotkey_capture_end(state: State<'_, Arc<AppState>>) -> Result<Option<Hotkey>> {
    Ok(state.end_hotkey_capture())
}

#[tauri::command]
pub async fn hotkey_set_paused(state: State<'_, Arc<AppState>>, paused: bool) -> Result<()> {
    ops::hotkey_set_paused(&state, paused)
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn permissions_status(
    state: State<'_, Arc<AppState>>,
) -> Result<std::collections::BTreeMap<String, &'static str>> {
    ops::permissions_status(&state)
}

#[tauri::command]
pub async fn permissions_request(
    state: State<'_, Arc<AppState>>,
    permission: String,
) -> Result<()> {
    ops::permissions_request(&state, &permission)
}

#[tauri::command]
pub async fn permissions_open_settings(
    state: State<'_, Arc<AppState>>,
    permission: String,
) -> Result<()> {
    ops::permissions_open_settings(&state, &permission)
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

/// The system accent colour, or `None` when the platform would not give one.
///
/// `None` is not an error: the stylesheet carries a fallback accent for exactly
/// this case, and reporting a failure here would put a red banner in front of
/// the user over a colour.
#[tauri::command]
pub async fn accent_color(app: AppHandle) -> Result<Option<AccentColor>> {
    Ok(app
        .try_state::<Arc<dyn wl_platform::Appearance>>()
        .and_then(|appearance| appearance.accent())
        .map(AccentColor::of))
}

/// Broadcast a change the OS reported. Mirrors [`publish_session`]: the
/// platform observer is the single publisher, so no window has to poll.
pub fn publish_accent(app: &AppHandle, accent: wl_platform::Rgb) {
    if let Err(e) = app.emit("system:accent", AccentColor::of(accent)) {
        tracing::warn!(error = %e, "could not broadcast system:accent");
    }
}

// ---------------------------------------------------------------------------
// Overlay, windows, lifecycle
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn overlay_action(state: State<'_, Arc<AppState>>, action: String) -> Result<()> {
    ops::overlay_action(&state, &action)
}

#[tauri::command]
pub async fn window_open(app: AppHandle, name: String) -> Result<()> {
    let window = crate::windows::WindowName::parse(&name)
        .ok_or_else(|| format!("unknown window `{name}`"))?;
    crate::windows::open(&app, window).map_err(|e| fail("Could not open that window", e))
}

/// TRY-020. `exit` sends `code: Some(0)`, which the run loop does not prevent,
/// and dropping managed state closes the stores and the database handle.
#[tauri::command]
pub async fn app_quit(app: AppHandle) -> Result<()> {
    tracing::info!("quit requested from the frontend");
    // Abandoning an in-flight recording explicitly means the audio is spooled
    // rather than lost with the process.
    if let Ok(state) = state(&app) {
        if let Some(pipeline) = state.pipeline() {
            pipeline.abort();
        }
    }
    app.exit(0);
    Ok(())
}
