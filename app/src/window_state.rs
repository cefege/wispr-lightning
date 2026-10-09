//! Remembered geometry of managed windows (SET-007).
//! The overlay is never recorded: its frame is
//! recomputed from the work area on every show.
//!
//! Stored as `{label: {x, y, width, height}}` in logical points. A missing or
//! malformed file is ignored, and the window opens centred.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

fn file() -> PathBuf {
    wl_core::paths::app_support_dir().join("window-state.json")
}

fn read_all(path: &Path) -> BTreeMap<String, Geometry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        tracing::warn!(error = %e, path = %path.display(), "ignoring malformed window state");
        BTreeMap::new()
    })
}

pub fn load(label: &str) -> Option<Geometry> {
    load_from(&file(), label)
}

fn load_from(path: &Path, label: &str) -> Option<Geometry> {
    read_all(path)
        .remove(label)
        .filter(|g| g.width > 0.0 && g.height > 0.0)
}

pub fn save(label: &str, geometry: Geometry) {
    save_to(&file(), label, geometry);
}

fn save_to(path: &Path, label: &str, geometry: Geometry) {
    let mut all = read_all(path);
    if all.get(label) == Some(&geometry) {
        return;
    }
    all.insert(label.to_string(), geometry);
    let result = serde_json::to_string_pretty(&all)
        .map_err(std::io::Error::other)
        .and_then(|text| std::fs::write(path, text));
    if let Err(e) = result {
        tracing::warn!(error = %e, label, "could not save window state");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEOMETRY: Geometry = Geometry {
        x: 10.0,
        y: 20.0,
        width: 860.0,
        height: 580.0,
    };

    #[test]
    fn round_trips_per_label_without_clobbering_others() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.json");
        save_to(&path, "settings", GEOMETRY);
        save_to(&path, "history", Geometry { x: 0.0, ..GEOMETRY });
        assert_eq!(load_from(&path, "settings"), Some(GEOMETRY));
        assert_eq!(load_from(&path, "history").map(|g| g.x), Some(0.0));
        assert_eq!(load_from(&path, "notes"), None);
    }

    #[test]
    fn malformed_or_degenerate_state_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.json");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load_from(&path, "settings"), None);

        // A zero-size window would open invisible.
        save_to(
            &path,
            "settings",
            Geometry {
                width: 0.0,
                ..GEOMETRY
            },
        );
        assert_eq!(load_from(&path, "settings"), None);
    }
}
