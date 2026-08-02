//! The IPC surface.
//!
//! Every command is thin on purpose: validate the argument, delegate to the
//! state, a store or the platform, and map the failure to a `String` the
//! frontend can show. No behaviour lives here. Anything that needs a decision
//! belongs in [`crate::state::AppState`] (settings), `wl_core` (data) or
//! [`crate::pipeline`] (recording) — the moment two commands make the same
//! decision independently they start to disagree, which is exactly the drift
//! `save_settings` exists to prevent.
//!
//! A few small DTOs live here because they are strictly IPC response shapes.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use wl_core::db::models::{DictionaryEntry, NoteEntry, TranscriptEntry};
use wl_core::db::CsvImport;
use wl_core::settings::{Hotkey, Settings};
use wl_platform::sound::Cue;
use wl_platform::{Permission, PermissionState};
use wl_providers::credentials::DEEPGRAM_API_KEY;

use crate::state::AppState;

/// Commands return `Result<T, String>`: the webview has no way to act on a
/// typed error, and a rendered message is what the UI displays either way.
type Result<T, E = String> = std::result::Result<T, E>;

/// Turn any error into a message, and log it — the frontend shows the user a
/// sentence, the log keeps the detail a support report needs.
fn fail(context: &'static str, error: impl std::fmt::Display) -> String {
    tracing::error!(error = %error, context, "command failed");
    format!("{context}: {error}")
}

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

/// Record that setup is complete, but only after every permission needed by
/// the current configuration is genuinely available.
///
/// This check belongs at the command boundary, not only in the wizard: a stale
/// webview, Return-key handler, or future UI must not persist “complete” while
/// the microphone, hotkey, injection, or enabled OCR path is unusable.
#[tauri::command]
pub async fn onboarding_complete(state: State<'_, Arc<AppState>>) -> Result<Settings> {
    let mut settings = state.settings();
    let missing = missing_required_permissions(&settings, state.platform.permissions.as_ref());
    if !missing.is_empty() {
        return Err(format!(
            "Finish granting {} before completing setup.",
            missing.join(", ")
        ));
    }
    settings.did_complete_onboarding = true;
    state.save_settings(settings)
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

/// Boundary mirror of [`wl_platform::audio::InputDevice`].
#[derive(Debug, Clone, Serialize)]
pub struct InputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Enumerate input devices for the settings picker and the Refresh button
/// (SET-032 / SET-035).
///
/// Deliberately does not emit `devices:changed`: this is a query, and a query
/// that publishes a change event feeds any window refetching on that event
/// straight back into itself. The event belongs to the pipeline, which raises
/// it from an actual device fault.
#[tauri::command]
pub async fn audio_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<InputDeviceInfo>> {
    let devices = state
        .audio
        .list_devices()
        .map_err(|e| fail("Could not list input devices", e))?;

    // TRY-006: the tray's device submenu follows every enumeration, so its
    // check mark cannot go stale after an unplug.
    if let Some(tray) = state.tray() {
        tray.set_devices(devices.clone());
    }

    Ok(devices
        .into_iter()
        .map(|d| InputDeviceInfo {
            id: d.id,
            name: d.name,
            is_default: d.is_default,
        })
        .collect())
}

/// Play one cue from `pack`, for the settings window's Preview button
/// (SET-081).
///
/// Applying the pack here rather than only on save is what makes the preview
/// honest: the user hears the pack they are pointing at, not the one that was
/// loaded when the window opened. Playback still respects the sound-effects
/// toggle — a preview that overrode it would be the one sound the user cannot
/// silence.
#[tauri::command]
pub async fn sound_preview(
    state: State<'_, Arc<AppState>>,
    pack: Option<String>,
    cue: String,
) -> Result<()> {
    let cue = match cue.as_str() {
        "start" => Cue::Start,
        "stop" => Cue::Stop,
        other => return Err(format!("unknown sound cue `{other}`")),
    };

    state
        .sound
        .set_pack(pack.as_deref())
        .map_err(|e| fail("Could not load that sound pack", e))?;
    state.sound.play(cue);
    Ok(())
}

/// Sound packs found on disk, for the SET-080 dropdown.
///
/// Includes the literal `default`; the settings UI filters it out because the
/// dropdown's first entry is already "Default" with a `nil` tag. An empty list
/// means the bundled sounds directory is missing, which SET-080 renders as
/// "Default" alone — a missing resource is not an error worth blocking on.
#[tauri::command]
pub async fn sound_packs(state: State<'_, Arc<AppState>>) -> Result<Vec<String>> {
    Ok(state.sound.available_packs())
}

// ---------------------------------------------------------------------------
// Deepgram
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DeepgramStatus {
    pub configured: bool,
}

#[tauri::command]
pub async fn deepgram_status(state: State<'_, Arc<AppState>>) -> Result<DeepgramStatus> {
    Ok(DeepgramStatus {
        configured: wl_providers::is_ready(&state.credentials),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct DeepgramHealth {
    pub ok: bool,
    pub message: String,
}

#[tauri::command]
pub async fn deepgram_health(state: State<'_, Arc<AppState>>) -> Result<DeepgramHealth> {
    let provider = state.provider.read().clone();
    Ok(match provider.health().await {
        Ok(()) => DeepgramHealth {
            ok: true,
            message: "Deepgram is reachable and configured".into(),
        },
        Err(error) => DeepgramHealth {
            ok: false,
            message: error.user_message(),
        },
    })
}

#[tauri::command]
pub async fn deepgram_key_save(state: State<'_, Arc<AppState>>, key: String) -> Result<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Enter a Deepgram API key.".into());
    }
    state
        .credentials
        .set(DEEPGRAM_API_KEY, key)
        .map_err(|error| fail("Could not save the API key", error))
}

#[tauri::command]
pub async fn deepgram_key_clear(state: State<'_, Arc<AppState>>) -> Result<()> {
    state
        .credentials
        .delete(DEEPGRAM_API_KEY)
        .map_err(|error| fail("Could not clear the API key", error))
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
    // A negative LIMIT means "unbounded" in SQLite and a negative OFFSET is
    // treated as zero, so clamping here keeps a frontend bug from turning into
    // a full table scan.
    state
        .history
        .entries(limit.max(0), offset.max(0))
        .map_err(|e| fail("Could not read history", e))
}

#[tauri::command]
pub async fn history_search(
    state: State<'_, Arc<AppState>>,
    query: String,
) -> Result<Vec<TranscriptEntry>> {
    state
        .history
        .search(&query)
        .map_err(|e| fail("Could not search history", e))
}

#[tauri::command]
pub async fn history_delete(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<()> {
    state
        .history
        .delete_entry(&id)
        .map_err(|e| fail("Could not delete that entry", e))?;
    let _ = app.emit("history:changed", ());
    Ok(())
}

#[tauri::command]
pub async fn history_clear(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<()> {
    state
        .history
        .clear_all()
        .map_err(|e| fail("Could not clear history", e))?;
    let _ = app.emit("history:changed", ());
    Ok(())
}

// ---------------------------------------------------------------------------
// Dictionary
// ---------------------------------------------------------------------------

/// `"vocabulary"` or `"snippets"`, the two tabs of the dictionary window
/// (WIN-033). One table backs both; `is_snippet` tells them apart.
fn parse_kind(kind: &str) -> Result<bool> {
    match kind {
        "vocabulary" => Ok(false),
        "snippets" => Ok(true),
        other => Err(format!("unknown dictionary kind `{other}`")),
    }
}

/// What the frontend sends for an add or an update.
///
/// Deliberately not the full row: the webview cannot know an id it has not been
/// given, and it must never dictate `created_at`, `modified_at`, `source` or
/// `frequency_used` — those are the store's to stamp. `id` is ignored on add
/// and required on update.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryInput {
    #[serde(default)]
    pub id: String,
    pub phrase: String,
    #[serde(default)]
    pub replacement: Option<String>,
    #[serde(default)]
    pub is_snippet: bool,
}

impl DictionaryInput {
    /// Trim, and treat a blank replacement as absent so it is stored as NULL
    /// (WIN-050).
    fn normalised(&self) -> Result<(String, Option<String>)> {
        let phrase = self.phrase.trim();
        if phrase.is_empty() {
            return Err("Enter a word or phrase.".to_string());
        }
        let replacement = self
            .replacement
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_string);
        Ok((phrase.to_string(), replacement))
    }
}

#[tauri::command]
pub async fn dictionary_list(
    state: State<'_, Arc<AppState>>,
    kind: String,
) -> Result<Vec<DictionaryEntry>> {
    let snippet = parse_kind(&kind)?;
    state
        .dictionary
        .entries(snippet)
        .map_err(|e| fail("Could not read the dictionary", e))
}

#[tauri::command]
pub async fn dictionary_add(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    entry: DictionaryInput,
) -> Result<DictionaryEntry> {
    let (phrase, replacement) = entry.normalised()?;

    let inserted = state
        .dictionary
        .add_manual(&phrase, replacement.as_deref(), entry.is_snippet)
        .map_err(|e| fail("Could not add that entry", e))?;
    let _ = app.emit("dictionary:changed", ());

    // `add_manual` is INSERT OR IGNORE against `UNIQUE(phrase, …)`, so a phrase
    // that already exists reports `false` and the existing row is returned
    // unchanged — which is what the user asked about either way.
    let stored = state
        .dictionary
        .entry_by_phrase(&phrase)
        .map_err(|e| fail("Could not read back that entry", e))?;

    match stored {
        Some(stored) => Ok(stored),
        // Nothing inserted and nothing live means the phrase survives only as a
        // soft-deleted row, which still occupies its unique slot. The Swift app
        // had the same dead end (data-spec §3.4) and simply did nothing; saying
        // so is the difference between a limitation and an apparent bug.
        None if !inserted => Err(format!(
            "\u{201c}{phrase}\u{201d} was deleted earlier and cannot be re-added under the same spelling."
        )),
        None => Err("The entry was not saved.".to_string()),
    }
}

#[tauri::command]
pub async fn dictionary_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    entry: DictionaryInput,
) -> Result<()> {
    if entry.id.is_empty() {
        return Err("Cannot update an entry without an id.".to_string());
    }
    let (phrase, replacement) = entry.normalised()?;

    state
        .dictionary
        .update_entry(&entry.id, &phrase, replacement.as_deref())
        .map_err(|e| fail("Could not save that entry", e))?;
    let _ = app.emit("dictionary:changed", ());
    Ok(())
}

#[tauri::command]
pub async fn dictionary_delete(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<()> {
    state
        .dictionary
        .soft_delete(&id)
        .map_err(|e| fail("Could not delete that entry", e))?;
    let _ = app.emit("dictionary:changed", ());
    Ok(())
}

/// Import a two-column CSV chosen through the native file dialog (WIN-047).
///
/// Returns the accepted count *and* the per-line errors: WIN-048's alert needs
/// both, and an import that silently drops eleven malformed rows while
/// reporting "Imported 40 entries" is worse than one that fails loudly.
#[tauri::command]
pub async fn dictionary_import_csv(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<CsvImport> {
    let report = state
        .dictionary
        .import_csv_file(std::path::Path::new(&path))
        .map_err(|e| fail("Could not import that file", e))?;
    let _ = app.emit("dictionary:changed", ());
    Ok(report)
}

// ---------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------

/// Notes, newest first. An empty or whitespace-only query lists everything,
/// matching the Swift view model's "empty query means no filter".
#[tauri::command]
pub async fn notes_list(
    state: State<'_, Arc<AppState>>,
    query: Option<String>,
) -> Result<Vec<NoteEntry>> {
    let query = query.unwrap_or_default();
    let result = if query.trim().is_empty() {
        state.notes.notes(NOTES_PAGE)
    } else {
        state.notes.search(&query)
    };
    result.map_err(|e| fail("Could not read notes", e))
}

/// Rows fetched for the unfiltered notes list. The Swift store used the same
/// cap; the list has no paging UI, so a larger number would only slow the first
/// paint.
const NOTES_PAGE: i64 = 100;

#[tauri::command]
pub async fn notes_add(
    state: State<'_, Arc<AppState>>,
    title: String,
    content: String,
) -> Result<NoteEntry> {
    // WIN-031: an empty note is savable, so nothing is validated here.
    let id = state
        .notes
        .add_note(&title, &content)
        .map_err(|e| fail("Could not create that note", e))?;

    state
        .notes
        .note(&id)
        .map_err(|e| fail("Could not read back that note", e))?
        .ok_or_else(|| "The note was not saved.".to_string())
}

#[tauri::command]
pub async fn notes_update(
    state: State<'_, Arc<AppState>>,
    id: String,
    title: String,
    content: String,
) -> Result<()> {
    state
        .notes
        .update_note(&id, &title, &content)
        .map_err(|e| fail("Could not save that note", e))
}

#[tauri::command]
pub async fn notes_delete(state: State<'_, Arc<AppState>>, id: String) -> Result<()> {
    state
        .notes
        .soft_delete(&id)
        .map_err(|e| fail("Could not delete that note", e))
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

/// TRY-016: pausing goes through the settings writer, so the tray item and the
/// stored `hotkeyPaused` can never disagree.
#[tauri::command]
pub async fn hotkey_set_paused(state: State<'_, Arc<AppState>>, paused: bool) -> Result<()> {
    let mut settings = state.settings();
    settings.hotkey_paused = paused;
    state.save_settings(settings).map(|_| ())
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

/// Wire names for [`Permission`]. Stable strings, because the frontend keys its
/// permission rows off them.
const PERMISSIONS: [(&str, Permission); 4] = [
    ("microphone", Permission::Microphone),
    ("accessibility", Permission::Accessibility),
    ("input_monitoring", Permission::InputMonitoring),
    ("screen_recording", Permission::ScreenRecording),
];

/// Permissions without which the current configuration cannot deliver a
/// dictation. Screen Recording joins the macOS set only while OCR is enabled.
fn required_permissions(settings: &Settings) -> Vec<(&'static str, Permission)> {
    #[cfg(target_os = "macos")]
    {
        let mut required = vec![
            ("Microphone", Permission::Microphone),
            ("Accessibility", Permission::Accessibility),
            ("Input Monitoring", Permission::InputMonitoring),
        ];
        if settings.use_screen_context {
            required.push(("Screen Recording", Permission::ScreenRecording));
        }
        required
    }
    #[cfg(target_os = "windows")]
    {
        let _ = settings;
        vec![("Microphone", Permission::Microphone)]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = settings;
        Vec::new()
    }
}

/// Required permissions still unavailable. `NotApplicable` is success: the
/// platform has explicitly said there is no grant for the user to make.
pub(crate) fn missing_required_permissions(
    settings: &Settings,
    permissions: &dyn wl_platform::Permissions,
) -> Vec<&'static str> {
    required_permissions(settings)
        .into_iter()
        .filter_map(|(name, permission)| {
            (!matches!(
                permissions.status(permission),
                PermissionState::Granted | PermissionState::NotApplicable
            ))
            .then_some(name)
        })
        .collect()
}

fn parse_permission(name: &str) -> Result<Permission> {
    PERMISSIONS
        .into_iter()
        .find(|(wire, _)| *wire == name)
        .map(|(_, permission)| permission)
        .ok_or_else(|| format!("unknown permission `{name}`"))
}

fn permission_state(state: PermissionState) -> &'static str {
    match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::NotDetermined => "not_determined",
        PermissionState::NotApplicable => "not_applicable",
    }
}

#[tauri::command]
pub async fn permissions_status(
    state: State<'_, Arc<AppState>>,
) -> Result<BTreeMap<String, &'static str>> {
    Ok(PERMISSIONS
        .into_iter()
        .map(|(wire, permission)| {
            (
                wire.to_string(),
                permission_state(state.platform.permissions.status(permission)),
            )
        })
        .collect())
}

#[tauri::command]
pub async fn permissions_request(
    state: State<'_, Arc<AppState>>,
    permission: String,
) -> Result<()> {
    state
        .platform
        .permissions
        .request(parse_permission(&permission)?);
    Ok(())
}

/// For the denied case, where no prompt will ever appear again and the only
/// route forward is the system settings pane.
#[tauri::command]
pub async fn permissions_open_settings(
    state: State<'_, Arc<AppState>>,
    permission: String,
) -> Result<()> {
    state
        .platform
        .permissions
        .open_settings(parse_permission(&permission)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

/// The system accent colour, everything derived from it that CSS cannot derive
/// for itself, and nothing else.
///
/// All `#rrggbb`, because the only consumer writes them straight into
/// `document.documentElement.style` as custom properties. `darker` and
/// `lighter` are the hover and pressed shades for the light and dark
/// appearances respectively — the stylesheet picks between them on
/// `prefers-color-scheme`, which is a fact the backend does not have and does
/// not need.
#[derive(Debug, Clone, Serialize)]
pub struct AccentColor {
    pub accent: String,
    pub text: String,
    pub darker: String,
    pub lighter: String,
}

impl AccentColor {
    fn of(accent: wl_platform::Rgb) -> Self {
        Self {
            accent: accent.to_hex(),
            text: accent.foreground().to_hex(),
            darker: accent.darker().to_hex(),
            lighter: accent.lighter().to_hex(),
        }
    }
}

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
    // The spelling lives with the enum, so the IPC name and the variant cannot
    // drift apart: `"dismiss"` maps to `Discard`.
    let action = crate::pipeline::OverlayAction::parse(&action)
        .ok_or_else(|| format!("unknown overlay action `{action}`"))?;

    let pipeline = state
        .pipeline()
        .ok_or_else(|| "the recording pipeline is not running".to_string())?;
    pipeline.overlay_action(action);
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    struct PermissionFixture {
        unavailable: Option<Permission>,
        fallback: PermissionState,
    }

    impl wl_platform::Permissions for PermissionFixture {
        fn status(&self, permission: Permission) -> PermissionState {
            if self.unavailable == Some(permission) {
                PermissionState::Denied
            } else {
                self.fallback
            }
        }

        fn request(&self, _permission: Permission) {}

        fn open_settings(&self, _permission: Permission) {}
    }

    #[test]
    fn not_applicable_permissions_never_block_setup() {
        let fixture = PermissionFixture {
            unavailable: None,
            fallback: PermissionState::NotApplicable,
        };
        assert!(missing_required_permissions(&Settings::default(), &fixture).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_setup_requires_ocr_permission_only_when_ocr_is_enabled() {
        let denied_screen = PermissionFixture {
            unavailable: Some(Permission::ScreenRecording),
            fallback: PermissionState::Granted,
        };
        let mut settings = Settings {
            use_screen_context: true,
            ..Default::default()
        };
        assert_eq!(
            missing_required_permissions(&settings, &denied_screen),
            vec!["Screen Recording"]
        );

        settings.use_screen_context = false;
        assert!(missing_required_permissions(&settings, &denied_screen).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_core_macos_permission_blocks_setup_when_missing() {
        for (permission, name) in [
            (Permission::Microphone, "Microphone"),
            (Permission::Accessibility, "Accessibility"),
            (Permission::InputMonitoring, "Input Monitoring"),
        ] {
            let fixture = PermissionFixture {
                unavailable: Some(permission),
                fallback: PermissionState::Granted,
            };
            assert_eq!(
                missing_required_permissions(&Settings::default(), &fixture),
                vec![name]
            );
        }
    }

    #[test]
    fn dictionary_kinds_map_to_the_is_snippet_column() {
        assert_eq!(parse_kind("vocabulary"), Ok(false));
        assert_eq!(parse_kind("snippets"), Ok(true));
        assert!(parse_kind("snippet").is_err());
        assert!(parse_kind("").is_err());
    }

    #[test]
    fn permission_names_round_trip() {
        for (wire, expected) in PERMISSIONS {
            assert_eq!(parse_permission(wire), Ok(expected));
        }
        assert!(parse_permission("camera").is_err());
    }

    #[test]
    fn permission_states_use_the_contract_spelling() {
        assert_eq!(permission_state(PermissionState::Granted), "granted");
        assert_eq!(permission_state(PermissionState::Denied), "denied");
        assert_eq!(
            permission_state(PermissionState::NotDetermined),
            "not_determined"
        );
        assert_eq!(
            permission_state(PermissionState::NotApplicable),
            "not_applicable"
        );
    }

    fn input(phrase: &str, replacement: Option<&str>) -> DictionaryInput {
        DictionaryInput {
            id: String::new(),
            phrase: phrase.to_string(),
            replacement: replacement.map(str::to_string),
            is_snippet: false,
        }
    }

    #[test]
    fn dictionary_input_trims_and_nulls_a_blank_replacement() {
        assert_eq!(
            input("  hello  ", Some("  world  ")).normalised(),
            Ok(("hello".to_string(), Some("world".to_string())))
        );
        // WIN-050: a blank replacement is stored as NULL, not as "".
        assert_eq!(
            input("hello", Some("   ")).normalised(),
            Ok(("hello".to_string(), None))
        );
        assert_eq!(
            input("hello", None).normalised(),
            Ok(("hello".to_string(), None))
        );
    }

    #[test]
    fn dictionary_input_rejects_a_blank_phrase() {
        assert!(input("", None).normalised().is_err());
        assert!(input("   \t ", None).normalised().is_err());
    }

    /// The frontend sends the whole row shape; only these four fields are read.
    #[test]
    fn dictionary_input_accepts_the_full_row_the_frontend_sends() {
        let json = r#"{
            "id": "",
            "phrase": "Wispr",
            "replacement": null,
            "isSnippet": false,
            "manualEntry": true,
            "source": null,
            "frequencyUsed": 0,
            "createdAt": 0,
            "modifiedAt": 0
        }"#;
        let parsed: DictionaryInput = serde_json::from_str(json).expect("deserialisable");
        assert_eq!(parsed.phrase, "Wispr");
        assert!(!parsed.is_snippet);
        assert_eq!(parsed.replacement, None);
    }
}
