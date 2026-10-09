//! Locations that depend on how the binary is installed.

use std::path::PathBuf;

use dioxus::prelude::*;

/// The sound packs, shipped through dx's asset pipeline. Config `resources`
/// cannot carry them: dx 0.7 copies those by bare file name, and the packs
/// share file names. A folder asset keeps its tree and is not hashed.
static SOUNDS: Asset = asset!("/resources/sounds", AssetOptions::folder());

/// The directory holding the `sounds/` packs.
///
/// A bundled app keeps its assets in `assets/` next to the executable on
/// Windows and Linux, and in `Contents/Resources/assets` on macOS. A
/// development build has no bundle, so it reads the source tree's copy under
/// this crate's `resources/` directory.
pub fn resource_dir() -> PathBuf {
    let bundled_name = SOUNDS.bundled().bundled_path().to_string();
    let installed = std::env::current_exe().ok().and_then(|exe| {
        let dir = exe.parent()?.to_path_buf();
        Some(if cfg!(target_os = "macos") {
            dir.join("../Resources/assets")
        } else {
            dir.join("assets")
        })
    });
    match installed {
        Some(dir) if dir.join(&bundled_name).is_dir() => dir,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"),
    }
}
