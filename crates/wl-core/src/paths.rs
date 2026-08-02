//! On-disk locations, resolved per platform.
//!
//! macOS keeps the historical `~/Library/Application Support/WisprLightning`
//! path so an existing install upgrades in place with its database, settings
//! and history intact.

use std::path::{Path, PathBuf};

/// Directory holding the database, settings and pending-audio spool.
pub fn app_support_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home()
            .join("Library")
            .join("Application Support")
            .join("WisprLightning")
    }
    #[cfg(not(target_os = "macos"))]
    {
        // %APPDATA%\WisprLightning on Windows; XDG data dir elsewhere.
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
            .unwrap_or_else(|| home().join(".local").join("share"))
            .join("WisprLightning")
    }
}

pub fn settings_file() -> PathBuf {
    app_support_dir().join("settings.json")
}

pub fn database_file() -> PathBuf {
    app_support_dir().join("lightning.db")
}

/// Pre-2.0 database name, renamed on first launch if present.
pub fn legacy_database_file() -> PathBuf {
    app_support_dir().join("history.db")
}

/// Spool for recordings that have not been transcribed yet, so a crash or a
/// failed retry never loses audio.
pub fn pending_audio_dir() -> PathBuf {
    app_support_dir().join("PendingAudio")
}

pub fn log_file() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home()
            .join("Library")
            .join("Logs")
            .join("WisprLightning.log")
    }
    #[cfg(not(target_os = "macos"))]
    {
        app_support_dir().join("WisprLightning.log")
    }
}

/// Create `dir` if it is missing. Errors are returned, not swallowed — the
/// Swift original discarded them and then failed opaquely much later.
pub fn ensure_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

fn home() -> PathBuf {
    #[cfg(windows)]
    let key = "USERPROFILE";
    #[cfg(not(windows))]
    let key = "HOME";
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_data_files_live_under_one_directory() {
        let root = app_support_dir();
        for p in [
            settings_file(),
            database_file(),
            legacy_database_file(),
            pending_audio_dir(),
        ] {
            assert!(p.starts_with(&root), "{p:?} escaped {root:?}");
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_keeps_the_historical_paths_so_installs_upgrade_in_place() {
        let d = app_support_dir();
        assert!(
            d.ends_with("Library/Application Support/WisprLightning"),
            "{d:?}"
        );
        assert!(log_file().ends_with("Library/Logs/WisprLightning.log"));
    }
}
