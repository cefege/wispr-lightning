//! Locations that depend on how the binary is installed.

use std::path::PathBuf;

/// The bundled `resources/` directory (sound packs).
///
/// Next to the executable on Windows and Linux; inside the bundle's
/// `Contents/Resources` on macOS. A development build has no bundle, so it
/// reads the source tree's copy under this crate's `resources/` directory.
pub fn resource_dir() -> PathBuf {
    let installed = std::env::current_exe().ok().and_then(|exe| {
        let dir = exe.parent()?.to_path_buf();
        Some(if cfg!(target_os = "macos") {
            dir.join("../Resources/resources")
        } else {
            dir.join("resources")
        })
    });
    match installed {
        Some(dir) if dir.is_dir() => dir,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"),
    }
}
