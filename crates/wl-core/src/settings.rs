//! User settings: the on-disk model, its defaults, and migration.
//!
//! Two hard requirements.
//!
//! **Back-compat.** An existing macOS install has a `settings.json` written by
//! the Swift app. Every key name and default here matches it exactly, so an
//! upgrade preserves the user's configuration rather than silently resetting
//! it.
//!
//! **No cliff-edge resets.** The Swift `load()` fell back to a *complete*
//! default `AppSettings` if decoding failed for any reason, so one renamed or
//! missing key wiped everything the user had configured. Every field here is
//! `#[serde(default)]`, so unknown keys are ignored and missing keys fall back
//! individually. See `PORT_PLAN.md` DV3.

use crate::fsm::PressBehavior;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub mod hotkey;
pub use hotkey::{Hotkey, Modifiers, TriggerKey};

/// Typing speed for Natural Mode, in characters per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TypingSpeed {
    Slow,
    #[default]
    Normal,
    Expert,
}

impl TypingSpeed {
    /// Characters per second. ~30, ~50 and ~80 words per minute.
    pub fn chars_per_second(self) -> f64 {
        match self {
            Self::Slow => 2.5,
            Self::Normal => 4.0,
            Self::Expert => 6.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailSignature {
    WrittenWithLightning,
    SpokenWithLightning,
}

impl EmailSignature {
    pub fn suffix(&self) -> &'static str {
        match self {
            Self::WrittenWithLightning => "\n\n\u{2014} Written with Wispr Lightning",
            Self::SpokenWithLightning => "\n\n\u{2014} Spoken with Wispr Lightning",
        }
    }
}

/// The complete user configuration.
///
/// Field names are the JSON keys. They are camelCase because that is what the
/// Swift implementation wrote, and changing them would orphan every existing
/// install's settings file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // -- Hotkeys ---------------------------------------------------------
    /// Portable dictation triggers. Each entry is an independent alternative,
    /// not a chord: "press this **or** that".
    pub hotkeys: Vec<Hotkey>,
    /// Legacy macOS Carbon virtual keycodes, migrated into `hotkeys` on load.
    #[serde(rename = "hotkeyKeyCodes")]
    pub legacy_hotkey_key_codes: Vec<u16>,
    #[serde(rename = "hotkeyPaused")]
    pub hotkey_paused: bool,
    /// Deprecated by the three-way [`Self::hotkey_press_behavior`] picker, but
    /// still written: the Swift build reads it, and its `Codable` decode fails
    /// outright on a missing key.
    #[serde(rename = "hotkeyTapToToggle")]
    pub hotkey_tap_to_toggle: bool,
    /// `"hold"` | `"toggle"` | `"legacy"`. Read through
    /// [`Self::press_behavior`], which resolves the empty string and the
    /// deprecated bool above.
    #[serde(rename = "hotkeyPressBehavior")]
    pub hotkey_press_behavior: String,

    // -- Audio -----------------------------------------------------------
    /// Stable device id (`coreaudio:<uid>` / `wasapi:<endpoint>`); `None` means
    /// the system default. Never a device *name* — two identical USB mics.
    #[serde(rename = "micDeviceId")]
    pub mic_device_id: Option<String>,
    /// Display label for the selected device, shown in the UI only.
    #[serde(rename = "micDeviceName")]
    pub mic_device_name: Option<String>,
    #[serde(rename = "keepMicrophoneActive")]
    pub keep_microphone_active: bool,
    #[serde(rename = "enableSounds")]
    pub enable_sounds: bool,
    #[serde(rename = "selectedSoundPack")]
    pub selected_sound_pack: Option<String>,
    #[serde(rename = "muteMusic")]
    pub mute_music: bool,

    // -- Deepgram --------------------------------------------------------
    /// Deepgram model id. `nova-3` is the only family supporting `keyterm`.
    #[serde(rename = "deepgramModel")]
    pub deepgram_model: String,
    /// Send the user's vocabulary as `keyterm` recognition hints.
    #[serde(rename = "deepgramKeytermBoost")]
    pub deepgram_keyterm_boost: bool,
    /// Deepgram's spoken punctuation and layout commands.
    #[serde(rename = "commandModeEnabled")]
    pub command_mode_enabled: bool,
    /// Extract keyterms from screen OCR when Nova 3 keyterm boosting is active.
    #[serde(rename = "useScreenContext")]
    pub use_screen_context: bool,
    /// Extract keyterms from the focused application and field text.
    #[serde(rename = "useAccessibilityContext")]
    pub use_accessibility_context: bool,
    #[serde(rename = "autoLearnWords")]
    pub auto_learn_words: bool,
    /// A BCP-47 code, `__auto__`, or `__multi__`.
    #[serde(rename = "deepgramLanguage")]
    pub deepgram_language: String,

    // -- Injection -------------------------------------------------------
    #[serde(rename = "naturalModeEnabled")]
    pub natural_mode_enabled: bool,
    #[serde(rename = "naturalModeSpeed")]
    pub natural_mode_speed: TypingSpeed,

    // -- Email -----------------------------------------------------------
    #[serde(rename = "emailAutoSignature")]
    pub email_auto_signature: bool,
    #[serde(rename = "emailSignatureOption")]
    pub email_signature_option: EmailSignature,

    // -- App -------------------------------------------------------------
    #[serde(rename = "launchAtLogin")]
    pub launch_at_login: bool,
    /// Show a Dock icon on macOS / a taskbar entry on Windows.
    #[serde(rename = "showInDock")]
    pub show_in_dock: bool,
    #[serde(rename = "shareUsageData")]
    pub share_usage_data: bool,
    #[serde(rename = "verboseLogging")]
    pub verbose_logging: bool,
    /// Set once the onboarding wizard has been completed. False re-shows it at
    /// launch, so losing this flag means every launch nags the user.
    #[serde(rename = "didCompleteOnboarding")]
    pub did_complete_onboarding: bool,

    // -- Forward compatibility -------------------------------------------
    /// Unknown keys survive a round trip unless migration explicitly removes
    /// a setting retired by the Deepgram-only cutover.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkeys: vec![Hotkey::modifier(Modifiers::CTRL_LEFT)],
            legacy_hotkey_key_codes: Vec::new(),
            hotkey_paused: false,
            hotkey_tap_to_toggle: false,
            hotkey_press_behavior: PressBehavior::default().as_setting().to_string(),

            mic_device_id: None,
            mic_device_name: None,
            keep_microphone_active: false,
            enable_sounds: true,
            selected_sound_pack: None,
            mute_music: false,

            deepgram_model: "nova-3".into(),
            deepgram_keyterm_boost: true,
            command_mode_enabled: true,
            use_screen_context: true,
            use_accessibility_context: true,
            auto_learn_words: true,
            deepgram_language: "en".into(),

            natural_mode_enabled: false,
            natural_mode_speed: TypingSpeed::Normal,

            email_auto_signature: false,
            email_signature_option: EmailSignature::WrittenWithLightning,

            launch_at_login: false,
            show_in_dock: false,
            share_usage_data: false,
            verbose_logging: false,
            did_complete_onboarding: false,
            unknown: BTreeMap::new(),
        }
    }
}

/// What happened while loading, so the UI can tell the user their settings were
/// recovered rather than silently pretending nothing went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// No settings file yet — first run.
    Fresh,
    Loaded,
    /// Legacy Carbon keycodes were translated into portable hotkeys.
    MigratedHotkeys,
    /// The primary was missing or unparsable and the settings came from the
    /// `.bak` snapshot instead. The user keeps their configuration; only
    /// changes made since the last successful load are gone.
    RestoredFromBackup,
    /// The file was unparsable and there was no usable backup. It has been
    /// moved aside and defaults applied.
    Recovered {
        backup: std::path::PathBuf,
    },
}

impl Settings {
    /// Which press behaviour the hotkey follows.
    ///
    /// Mirrors the Swift load-time read exactly, including its one subtlety:
    /// an *empty* string means the user's file predates the three-way picker,
    /// so the deprecated `hotkeyTapToToggle` bool is the only record of what
    /// they chose. A *missing* key is not the same thing — it defaults to
    /// `"legacy"` like every other field.
    pub fn press_behavior(&self) -> PressBehavior {
        if self.hotkey_press_behavior.is_empty() {
            if self.hotkey_tap_to_toggle {
                PressBehavior::TapToToggle
            } else {
                PressBehavior::Legacy
            }
        } else {
            PressBehavior::from_setting(&self.hotkey_press_behavior)
        }
    }

    /// Set the press behaviour, keeping the deprecated `hotkeyTapToToggle`
    /// bool in sync.
    ///
    /// The Swift settings window writes both on every save. Letting them
    /// disagree would silently change the behaviour of a build that reads only
    /// the bool, so the mirror is maintained here rather than at each caller.
    pub fn set_press_behavior(&mut self, behavior: PressBehavior) {
        self.hotkey_press_behavior = behavior.as_setting().to_string();
        self.hotkey_tap_to_toggle = behavior == PressBehavior::TapToToggle;
    }

    /// Read from `path`, applying migrations. Never fails destructively.
    ///
    /// Three tiers, in order: the file itself, the `.bak` snapshot of the last
    /// file that parsed, then defaults. Without the middle tier the first
    /// corruption is unrecoverable — the port used to move the bad file aside
    /// and hand the user a factory reset.
    pub fn load(path: &Path) -> (Self, LoadOutcome) {
        let Ok(raw) = std::fs::read_to_string(path) else {
            // No file at all: first run, or someone deleted it. The snapshot
            // is still worth trying before resetting the user to defaults.
            return match Self::from_backup(path) {
                Some(s) => (s, LoadOutcome::RestoredFromBackup),
                None => (Self::default(), LoadOutcome::Fresh),
            };
        };

        match serde_json::from_str::<Self>(&raw) {
            Ok(mut s) => {
                // Snapshot only bytes that have just been proven to parse, and
                // only before migration, so the backup stays a faithful copy
                // of what is on disk.
                Self::write_backup(path, &raw);
                let migrated = s.migrate();
                (
                    s,
                    if migrated {
                        LoadOutcome::MigratedHotkeys
                    } else {
                        LoadOutcome::Loaded
                    },
                )
            }
            Err(err) => {
                tracing::error!(%err, "settings file is unparsable; backing it up");
                let backup = path.with_extension("json.corrupt");
                let _ = std::fs::rename(path, &backup);
                match Self::from_backup(path) {
                    Some(s) => {
                        tracing::warn!(
                            corrupt = %backup.display(),
                            "settings.json was unreadable; restored from the snapshot"
                        );
                        (s, LoadOutcome::RestoredFromBackup)
                    }
                    None => (Self::default(), LoadOutcome::Recovered { backup }),
                }
            }
        }
    }

    /// The `.bak` snapshot, migrated, if it exists and still parses.
    fn from_backup(path: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(path.with_extension("json.bak")).ok()?;
        let mut s = serde_json::from_str::<Self>(&raw).ok()?;
        s.migrate();
        Some(s)
    }

    /// Refresh the `.bak` snapshot from bytes known to parse.
    ///
    /// Written to a sibling tmp and renamed, so there is never an instant
    /// where the backup is half-written: a crash there would otherwise take
    /// out the copy and the original in one go.
    fn write_backup(path: &Path, raw: &str) {
        let tmp = path.with_extension("json.bak.tmp");
        let written = std::fs::write(&tmp, raw)
            .and_then(|()| std::fs::rename(&tmp, path.with_extension("json.bak")));
        if let Err(err) = written {
            // An unwritable backup is not a reason to fail the load; the user
            // still gets their settings, just without the safety net.
            tracing::warn!(%err, "could not refresh the settings backup");
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Translate anything carried over from an older on-disk shape. Returns
    /// whether something changed.
    pub fn migrate(&mut self) -> bool {
        let mut changed = false;

        if !self.legacy_hotkey_key_codes.is_empty() {
            let migrated: Vec<_> = self
                .legacy_hotkey_key_codes
                .iter()
                .filter_map(|&c| Hotkey::from_carbon_keycode(c))
                .collect();
            if !migrated.is_empty() {
                self.hotkeys = migrated;
                changed = true;
            }
            // Deliberately NOT cleared. The Swift build still reads these,
            // so dropping them here would wipe its hotkey configuration the
            // first time this app saves. They are consumed, not owned.
        }

        // A hotkey list emptied by a failed migration would leave the app with
        // no way to dictate at all.
        if self.hotkeys.is_empty() {
            self.hotkeys = Self::default().hotkeys;
            changed = true;
        }

        // One-time Deepgram-only cutover. Preserve the old shared language
        // choice, then discard settings for providers and services that no
        // longer exist in this build.
        if let Some(serde_json::Value::Array(languages)) = self.unknown.remove("languages") {
            let selected: Vec<_> = languages
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .filter(|value| !value.trim().is_empty())
                .collect();
            if self.deepgram_language.trim().is_empty() || self.deepgram_language == "en" {
                if selected.len() > 1 {
                    self.deepgram_language = "__multi__".into();
                } else if let Some(language) = selected.into_iter().next() {
                    self.deepgram_language = language;
                }
            }
            changed = true;
        }
        for retired in [
            "activeVendor",
            "provider",
            "fallbackChain",
            "openRouterModel",
            "aiFormatting",
            "autoCleanupLevel",
            "localPostProcessing",
            "styleDetectionEnabled",
            "personalizationStyles",
            "hyperlinkOn",
            "creatorMode",
            "polishEnabled",
            "polishInstructions",
            "autoPolish",
            "polish_hotkeys",
            "polishHotkeyKeyCodes",
        ] {
            changed |= self.unknown.remove(retired).is_some();
        }

        // B-015 left `hotkeyPressBehavior` empty in files written by the
        // tap-to-toggle-only build. Resolve it from the deprecated bool once,
        // so nothing downstream has to keep asking.
        if self.hotkey_press_behavior.is_empty() {
            let resolved = if self.hotkey_tap_to_toggle {
                PressBehavior::TapToToggle
            } else {
                PressBehavior::Legacy
            };
            self.set_press_behavior(resolved);
            changed = true;
        }

        changed
    }

    /// Write atomically. The Swift version used a plain non-atomic write and
    /// swallowed every error, so a crash mid-write truncated the file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Sync the deprecated `hotkeyTapToToggle` mirror here, at the one point
        // where settings become bytes, rather than trusting every caller to
        // route writes through `set_press_behavior`. They do not: the settings
        // pane assigns `hotkeyPressBehavior` directly, which left the bool
        // stale on disk. It matters because a Swift build reads only the bool,
        // so a file saying `"toggle"` with `false` gives that build
        // push-to-talk while this app's picker shows tap-to-toggle.
        //
        // Only the bool is touched. `set_press_behavior` would also rewrite the
        // string to its canonical tag, and that is not ours to do on save: a
        // value written by a newer build must survive a save by this one, and
        // even a recognised alias is the user's bytes, not a defect to correct.
        let mut out = self.clone();
        out.hotkey_tap_to_toggle = self.press_behavior() == PressBehavior::TapToToggle;

        let json = serde_json::to_vec_pretty(&out)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_deepgram_only() {
        let settings = Settings::default();
        assert_eq!(settings.deepgram_model, "nova-3");
        assert_eq!(settings.deepgram_language, "en");
        assert!(settings.deepgram_keyterm_boost);
    }

    #[test]
    fn migration_preserves_the_old_language_then_removes_retired_keys() {
        let mut settings: Settings = serde_json::from_value(serde_json::json!({
            "activeVendor": "wispr_flow",
            "languages": ["de"],
            "deepgramLanguage": "",
            "aiFormatting": false,
            "enableSounds": false
        }))
        .unwrap();

        assert!(settings.migrate());
        assert_eq!(settings.deepgram_language, "de");
        assert!(!settings.enable_sounds);
        assert!(!settings.unknown.contains_key("activeVendor"));
        assert!(!settings.unknown.contains_key("languages"));
        assert!(!settings.unknown.contains_key("aiFormatting"));
    }

    #[test]
    fn unknown_future_keys_survive_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings::default();
        settings
            .unknown
            .insert("futureOption".into(), serde_json::json!({"enabled": true}));
        settings.save(&path).unwrap();

        let (loaded, outcome) = Settings::load(&path);
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(
            loaded.unknown.get("futureOption"),
            Some(&serde_json::json!({"enabled": true}))
        );
    }
}
