//! Recording cues, played through one long-lived rodio mixer.
//!
//! Two rules drive the whole file, both learned the hard way:
//!
//! 1. **Exactly one output device is opened, at startup, and held for the life
//!    of the app.** Dropping a `MixerDeviceSink` stops everything playing
//!    through it, and opening a device per chime costs tens of milliseconds on
//!    the hotkey path.
//! 2. **Cues go to [`Mixer::add`], never to a `Player`.** A `Player` is a
//!    queue: press-and-release fast enough and the stop chime would wait for
//!    the start chime to finish. The mixer overlaps them, which is what the
//!    user expects.
//!
//! Sound packs are subdirectories of `Sounds/`, exactly as in the Swift
//! original: a file missing from a custom pack falls back to `default`.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use rodio::mixer::Mixer;
use rodio::source::Buffered;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Source};

use crate::sound::{Cue, SoundPlayer};
use crate::{PlatformError, Result};

/// The pack that ships with the app and is guaranteed complete. Every lookup
/// falls back here.
const DEFAULT_PACK: &str = "default";

/// A cue decoded once and replayable forever. `Buffered` clones share the
/// decoded samples, so a clone per play is a pointer bump.
type CueSource = Buffered<Decoder<Cursor<Vec<u8>>>>;

/// Filenames to try for a cue, in order.
///
/// The Swift app only ever loaded three names — `dictation-start.wav`,
/// `dictation-stop.wav` and `paste.wav` — and had no error cue at all; it
/// showed an overlay instead. `paste.wav` was loaded but never played, so it is
/// not carried over. [`Cue::Error`] uses `Notification.wav` where a pack ships
/// one (v1–v3 do, `default` does not) and otherwise reuses the stop chime,
/// which is better than silence for a failure the user needs to notice.
fn candidates(cue: Cue) -> &'static [&'static str] {
    match cue {
        Cue::Start => &["dictation-start.wav"],
        Cue::Stop => &["dictation-stop.wav"],
        Cue::Error => &["Notification.wav", "dictation-stop.wav"],
    }
}

const CUES: [Cue; 3] = [Cue::Start, Cue::Stop, Cue::Error];

fn slot(cue: Cue) -> usize {
    match cue {
        Cue::Start => 0,
        Cue::Stop => 1,
        Cue::Error => 2,
    }
}

/// Locate the file backing `cue`, preferring `pack` and falling back to the
/// built-in pack for anything it does not ship.
///
/// `None` means neither the pack nor `default` has any candidate — a broken
/// installation, not a user error.
fn resolve(root: &Path, pack: Option<&str>, cue: Cue) -> Option<PathBuf> {
    for name in candidates(cue) {
        if let Some(pack) = pack {
            let path = root.join(pack).join(name);
            if path.is_file() {
                return Some(path);
            }
        }
        let fallback = root.join(DEFAULT_PACK).join(name);
        if fallback.is_file() {
            return Some(fallback);
        }
    }
    None
}

/// Packs on disk, alphabetically. Always non-empty: a missing or empty `Sounds`
/// directory still reports `default`, so the settings picker never shows an
/// empty list.
fn discover_packs(root: &Path) -> Vec<String> {
    let mut packs: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    packs.sort();
    if packs.is_empty() {
        packs.push(DEFAULT_PACK.to_owned());
    }
    packs
}

/// Read and decode one cue file into memory.
fn decode(path: &Path) -> Result<CueSource> {
    let bytes = std::fs::read(path)?;
    let decoder = Decoder::new_wav(Cursor::new(bytes))
        .map_err(|e| PlatformError::Other(format!("cannot decode {}: {e}", path.display())))?;
    let buffered = decoder.buffered();
    // `Buffered` decodes lazily and caches. Draining a clone now moves that
    // cost off the hotkey path — the first chime would otherwise decode a
    // 130 KB WAV while the user is waiting to see the overlay.
    buffered.clone().for_each(drop);
    Ok(buffered)
}

/// The cue set for one pack. `None` in a slot means no file resolved; that cue
/// is silently skipped rather than failing the whole player.
#[derive(Default)]
struct Loaded {
    cues: [Option<CueSource>; 3],
}

impl Loaded {
    fn for_pack(root: &Path, pack: Option<&str>) -> Self {
        let mut loaded = Self::default();
        for cue in CUES {
            let Some(path) = resolve(root, pack, cue) else {
                tracing::warn!(?cue, ?pack, "no sound file found for cue");
                continue;
            };
            match decode(&path) {
                Ok(source) => loaded.cues[slot(cue)] = Some(source),
                Err(e) => tracing::warn!(%e, ?cue, "sound cue will be silent"),
            }
        }
        loaded
    }
}

/// The open output device and the mixer that feeds it.
///
/// `None` on a machine with no usable output. That is a degradation, never an
/// error: a dictation app whose chimes are silent still dictates, so refusing
/// to start would trade a missing sound for a missing feature.
struct Output {
    /// Held for the life of the app. Dropping it silences every cue, which is
    /// why it is stored rather than bound to a local at startup.
    _device: MixerDeviceSink,
    mixer: Mixer,
}

pub struct RodioPlayer {
    output: Option<Output>,
    root: PathBuf,
    loaded: Mutex<Loaded>,
    enabled: AtomicBool,
}

impl RodioPlayer {
    /// `root` is the directory containing the pack subdirectories.
    ///
    /// Never fails. A missing output device or a missing pack directory yields
    /// a player whose `play` is a no-op and whose other methods still behave,
    /// so callers hold one `Arc<dyn SoundPlayer>` unconditionally instead of
    /// each writing their own null implementation.
    pub fn new(root: PathBuf) -> Self {
        let output = match DeviceSinkBuilder::open_default_sink() {
            Ok(mut device) => {
                // Deliberate lifetime, so rodio's "you dropped the sink"
                // warning at shutdown is noise rather than a hint.
                device.log_on_drop(false);
                let mixer = device.mixer().clone();
                Some(Output {
                    _device: device,
                    mixer,
                })
            }
            Err(e) => {
                tracing::warn!(%e, "no audio output device; cues will be silent");
                None
            }
        };

        let loaded = Loaded::for_pack(&root, None);
        Self {
            output,
            root,
            loaded: Mutex::new(loaded),
            enabled: AtomicBool::new(true),
        }
    }

    /// Whether an output device was opened. For the probe and diagnostics; the
    /// app does not branch on it.
    pub fn is_available(&self) -> bool {
        self.output.is_some()
    }
}

impl SoundPlayer for RodioPlayer {
    fn play(&self, cue: Cue) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(output) = self.output.as_ref() else {
            return;
        };
        // Clone out from under the lock: `Mixer::add` wraps the source in a
        // resampling iterator, and holding the lock across it would serialize
        // a start and stop chime that are supposed to overlap.
        let source = self.loaded.lock().cues[slot(cue)].clone();
        match source {
            Some(source) => output.mixer.add(source),
            // No system-sound fallback: `NSSound(named:)` has no Windows
            // equivalent, and the shipped `default` pack always resolves.
            None => tracing::debug!(?cue, "cue has no sound file; skipping"),
        }
    }

    fn set_pack(&self, pack: Option<&str>) -> Result<()> {
        // Decode before taking the lock so `play` is never blocked on file I/O.
        let next = Loaded::for_pack(&self.root, pack);
        *self.loaded.lock() = next;
        Ok(())
    }

    fn available_packs(&self) -> Vec<String> {
        discover_packs(&self.root)
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}

/// The cue player for this build.
///
/// `resource_dir` is the bundle's resource root; the packs live in a `sounds`
/// subdirectory of it. Both casings are accepted because the repository ships
/// `Resources/Sounds` and bundlers are inconsistent about preserving case.
///
/// Infallible by design: neither a missing resource directory nor a missing
/// output device is something the caller can act on, and a `Result` here would
/// only push a null `SoundPlayer` implementation into every caller's crate.
pub fn player(resource_dir: PathBuf) -> Arc<dyn SoundPlayer> {
    let root = ["sounds", "Sounds"]
        .iter()
        .map(|name| resource_dir.join(name))
        .find(|path| path.is_dir())
        .unwrap_or_else(|| {
            tracing::warn!(
                dir = %resource_dir.display(),
                "no sounds directory in the bundle; cues will be silent"
            );
            resource_dir.join("sounds")
        });
    Arc::new(RodioPlayer::new(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Sounds` tree shaped like the one that ships: a complete `default`
    /// pack and a custom pack missing the start chime.
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for (pack, files) in [
            (
                "default",
                &["dictation-start.wav", "dictation-stop.wav", "paste.wav"][..],
            ),
            (
                "v2",
                &["dictation-stop.wav", "Notification.wav", "paste.wav"][..],
            ),
            ("aardvark", &[][..]),
        ] {
            std::fs::create_dir_all(root.join(pack)).unwrap();
            for file in files {
                std::fs::write(root.join(pack).join(file), b"").unwrap();
            }
        }
        dir
    }

    #[test]
    fn a_custom_pack_supplies_the_files_it_has() {
        let dir = fixture();
        let stop = resolve(dir.path(), Some("v2"), Cue::Stop).unwrap();
        assert_eq!(stop, dir.path().join("v2").join("dictation-stop.wav"));
    }

    #[test]
    fn a_file_missing_from_a_custom_pack_falls_back_to_the_built_in_pack() {
        // v2 has no dictation-start.wav. Selecting it must not silence the
        // start chime.
        let dir = fixture();
        let start = resolve(dir.path(), Some("v2"), Cue::Start).unwrap();
        assert_eq!(
            start,
            dir.path().join("default").join("dictation-start.wav")
        );
    }

    #[test]
    fn a_pack_with_no_files_at_all_still_resolves_every_cue() {
        let dir = fixture();
        for cue in CUES {
            assert!(
                resolve(dir.path(), Some("aardvark"), cue).is_some(),
                "{cue:?} fell through to nothing"
            );
        }
    }

    #[test]
    fn the_error_cue_prefers_a_dedicated_sound_and_borrows_the_stop_chime_otherwise() {
        let dir = fixture();
        assert_eq!(
            resolve(dir.path(), Some("v2"), Cue::Error).unwrap(),
            dir.path().join("v2").join("Notification.wav")
        );
        // `default` ships no Notification.wav, so the second candidate wins.
        assert_eq!(
            resolve(dir.path(), None, Cue::Error).unwrap(),
            dir.path().join("default").join("dictation-stop.wav")
        );
    }

    #[test]
    fn an_unknown_pack_name_behaves_like_the_built_in_pack() {
        // A pack removed between launches must not silence the app.
        let dir = fixture();
        assert_eq!(
            resolve(dir.path(), Some("deleted-by-the-user"), Cue::Start).unwrap(),
            dir.path().join("default").join("dictation-start.wav")
        );
    }

    #[test]
    fn a_broken_installation_resolves_to_nothing_rather_than_a_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve(dir.path(), None, Cue::Start).is_none());
    }

    #[test]
    fn packs_are_listed_alphabetically_and_exclude_loose_files() {
        let dir = fixture();
        std::fs::write(dir.path().join("README.txt"), b"").unwrap();
        assert_eq!(discover_packs(dir.path()), ["aardvark", "default", "v2"]);
    }

    #[test]
    fn a_missing_sounds_directory_still_offers_the_built_in_pack() {
        // The picker must never render an empty list.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(discover_packs(&dir.path().join("absent")), ["default"]);
        assert_eq!(discover_packs(dir.path()), ["default"]);
    }

    #[test]
    fn a_bundle_with_no_sounds_at_all_still_yields_a_usable_player() {
        // The whole reason `player` is infallible: callers hold one
        // `Arc<dyn SoundPlayer>` unconditionally. Every method must answer, and
        // `play` must be a no-op rather than a panic, on a broken install.
        let dir = tempfile::tempdir().unwrap();
        let player = player(dir.path().to_path_buf());

        player.play(Cue::Start);
        player.set_pack(Some("v2")).unwrap();
        player.play(Cue::Error);
        player.set_enabled(false);
        player.play(Cue::Stop);
        assert_eq!(player.available_packs(), ["default"]);
    }

    #[test]
    fn the_repository_ships_every_file_the_cue_table_names() {
        // Guards against renaming an asset without updating `candidates`.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../Resources/Sounds")
            .canonicalize()
            .expect("Resources/Sounds is part of the repository");
        for pack in discover_packs(&root) {
            for cue in CUES {
                assert!(
                    resolve(&root, Some(&pack), cue).is_some(),
                    "pack {pack} cannot play {cue:?}"
                );
            }
        }
    }
}
