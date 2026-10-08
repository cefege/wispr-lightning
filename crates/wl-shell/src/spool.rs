//! Crash-safe storage for recordings that have not been transcribed yet.
//!
//! A dictation the user already spoke is unrecoverable if it is lost — they
//! have to say the whole thing again, and they usually cannot remember it
//! verbatim. So audio is written to disk the moment recording stops, before
//! the network is involved, and only deleted once a transcript comes back.
//!
//! The on-disk format is a bare concatenation of fixed-size packets. No
//! header: the packet size is a protocol constant, so the file length alone
//! determines the framing, and a truncated write still yields every complete
//! packet that made it to disk.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use wl_core::consts::{CHUNK_BYTES, MIN_PACKETS};

/// Recordings older than this are abandoned rather than offered for retry: a
/// day-old dictation has almost certainly been retyped by hand already, and
/// silently pasting it into whatever is focused now would be alarming.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Writes and recovers pending recordings under one directory.
pub struct Spool {
    dir: PathBuf,
}

pub struct Recovered {
    pub path: PathBuf,
    pub packets: Vec<Vec<i16>>,
}

impl Spool {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Persist `packets`, returning the file written.
    ///
    /// Writes to a temporary name and renames, so a crash mid-write cannot
    /// leave a half-file that recovery would later mistake for a real
    /// recording.
    pub fn save(&self, packets: &[Vec<i16>]) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let bytes = to_pcm(packets);

        // Millisecond precision, not seconds: two dictations a moment apart
        // would otherwise collide on one filename and the second would
        // silently overwrite audio the first is still waiting to send.
        let path = self.dir.join(format!(
            "recording-{}-{:03}.pcm",
            timestamp(),
            subsec_millis()
        ));
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
        tracing::info!(
            packets = packets.len(),
            kb = bytes.len() / 1024,
            file = %path.display(),
            "spooled recording"
        );
        Ok(path)
    }

    pub fn delete(&self, path: &Path) {
        match std::fs::remove_file(path) {
            Ok(()) => tracing::debug!(file = %path.display(), "removed spooled recording"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(file = %path.display(), error = %e, "could not remove spooled recording");
            }
        }
    }

    /// Read a spooled file back into packets. A trailing partial packet is
    /// discarded: it can only come from a truncated write.
    pub fn load(path: &Path) -> std::io::Result<Vec<Vec<i16>>> {
        let bytes = std::fs::read(path)?;
        Ok(bytes
            .chunks_exact(CHUNK_BYTES)
            .map(|chunk| {
                chunk
                    .chunks_exact(2)
                    .map(|s| i16::from_le_bytes([s[0], s[1]]))
                    .collect()
            })
            .collect())
    }

    /// Find the most recent recoverable recording, discarding everything else.
    ///
    /// Only one is offered: the user can act on one recovery prompt, and
    /// queueing several would paste a backlog of old dictations into whatever
    /// they happen to be doing now.
    pub fn recover_latest(&self) -> Option<Recovered> {
        let entries: Vec<(PathBuf, SystemTime)> = std::fs::read_dir(&self.dir)
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "pcm"))
            .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
            .collect();

        let (newest, modified) = entries.iter().max_by_key(|(_, t)| *t)?.clone();

        // Everything except the newest is unconditionally garbage.
        for (path, _) in &entries {
            if path != &newest {
                self.delete(path);
            }
        }

        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO);
        if age > MAX_AGE {
            tracing::info!(file = %newest.display(), "discarding stale spooled recording");
            self.delete(&newest);
            return None;
        }

        match Self::load(&newest) {
            Ok(packets) if packets.len() >= MIN_PACKETS => {
                tracing::info!(packets = packets.len(), "recovered unsent recording");
                Some(Recovered {
                    path: newest,
                    packets,
                })
            }
            Ok(_) => {
                // Too short to be worth transcribing; a live recording of this
                // length would be discarded by the same gate.
                self.delete(&newest);
                None
            }
            Err(e) => {
                tracing::warn!(file = %newest.display(), error = %e, "unreadable spool file");
                self.delete(&newest);
                None
            }
        }
    }

    /// Export a recording as a playable WAV, for the "Save" escape hatch in
    /// the recovery UI.
    pub fn export_wav(packets: &[Vec<i16>], dest: &Path) -> std::io::Result<()> {
        std::fs::write(dest, wl_core::wav::wrap_pcm(&to_pcm(packets)))
    }

    /// Suggested filename for an exported recording.
    pub fn export_filename() -> String {
        format!("wispr-recording-{}.wav", timestamp())
    }
}

fn to_pcm(packets: &[Vec<i16>]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(packets.len() * CHUNK_BYTES);
    for packet in packets {
        for sample in packet {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    bytes
}

/// Sub-second component of the current time, used only to disambiguate two
/// spool files written inside the same second.
fn subsec_millis() -> u32 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .subsec_millis()
}

/// Sortable, filename-safe UTC timestamp: `YYYYMMDD-HHMMSS`.
fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    // Civil-date conversion from days since the epoch (Howard Hinnant's
    // algorithm). Avoids a date-formatting dependency for one call site.
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wl_core::consts::CHUNK_SAMPLES;

    fn packets(n: usize) -> Vec<Vec<i16>> {
        (0..n).map(|i| vec![i as i16; CHUNK_SAMPLES]).collect()
    }

    fn spool() -> (tempfile::TempDir, Spool) {
        let dir = tempfile::tempdir().unwrap();
        let s = Spool::new(dir.path().join("PendingAudio"));
        (dir, s)
    }

    /// Set modification time without pulling in `filetime` for a test-only need.
    fn set_mtime(path: &Path, when: SystemTime) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_times(
            std::fs::FileTimes::new()
                .set_modified(when)
                .set_accessed(when),
        )
        .unwrap();
    }

    #[test]
    fn saved_audio_round_trips_exactly() {
        let (_d, s) = spool();
        let original = packets(7);
        let path = s.save(&original).unwrap();
        assert_eq!(Spool::load(&path).unwrap(), original);
    }

    #[test]
    fn saving_creates_the_directory_if_it_is_missing() {
        let (_d, s) = spool();
        assert!(!s.dir().exists());
        s.save(&packets(5)).unwrap();
        assert!(s.dir().exists());
    }

    #[test]
    fn no_partial_file_is_left_behind() {
        let (_d, s) = spool();
        s.save(&packets(5)).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(s.dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "part"))
            .collect();
        assert!(leftovers.is_empty(), "a .part file survived the save");
    }

    #[test]
    fn a_truncated_file_yields_its_complete_packets_and_drops_the_rest() {
        let (_d, s) = spool();
        let path = s.save(&packets(3)).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.truncate(CHUNK_BYTES * 2 + 100); // cut off mid-packet
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(Spool::load(&path).unwrap().len(), 2);
    }

    #[test]
    fn recovery_returns_the_newest_and_deletes_every_other_file() {
        let (_d, s) = spool();
        let old = s.save(&packets(6)).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let newest = s.save(&packets(9)).unwrap();
        let now = SystemTime::now();
        set_mtime(&old, now - Duration::from_secs(600));
        set_mtime(&newest, now);

        let recovered = s.recover_latest().expect("expected a recovery");
        assert_eq!(recovered.packets.len(), 9);
        assert_eq!(recovered.path, newest);
        assert!(!old.exists(), "older spool files must be cleaned up");
    }

    #[test]
    fn recovery_ignores_recordings_older_than_a_day() {
        let (_d, s) = spool();
        let path = s.save(&packets(9)).unwrap();
        set_mtime(&path, SystemTime::now() - Duration::from_secs(25 * 3600));
        assert!(s.recover_latest().is_none());
        assert!(!path.exists(), "a stale recording must be cleaned up");
    }

    #[test]
    fn recovery_skips_recordings_too_short_to_transcribe() {
        let (_d, s) = spool();
        let path = s.save(&packets(MIN_PACKETS - 3)).unwrap();
        assert!(s.recover_latest().is_none());
        assert!(!path.exists());
    }

    #[test]
    fn recovery_on_an_empty_or_missing_directory_is_none_not_an_error() {
        let (_d, s) = spool();
        assert!(s.recover_latest().is_none());
        std::fs::create_dir_all(s.dir()).unwrap();
        assert!(s.recover_latest().is_none());
    }

    #[test]
    fn deleting_a_file_that_is_already_gone_is_not_an_error() {
        let (_d, s) = spool();
        let path = s.save(&packets(5)).unwrap();
        s.delete(&path);
        s.delete(&path);
        assert!(!path.exists());
    }

    #[test]
    fn exported_wav_has_a_valid_header_and_the_full_payload() {
        let (d, _s) = spool();
        let dest = d.path().join("out.wav");
        Spool::export_wav(&packets(3), &dest).unwrap();
        let bytes = std::fs::read(&dest).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(bytes.len(), wl_core::wav::HEADER_LEN + 3 * CHUNK_BYTES);
    }

    #[test]
    fn timestamps_are_sortable_and_filename_safe() {
        let t = timestamp();
        assert_eq!(t.len(), 15, "YYYYMMDD-HHMMSS, got {t}");
        assert!(t.chars().all(|c| c.is_ascii_digit() || c == '-'), "{t}");
        assert!(Spool::export_filename().ends_with(".wav"));
    }
}
