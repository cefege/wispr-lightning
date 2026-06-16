import Foundation

/// Owns the PCM file for one dictation from creation through retirement.
/// Replaces a previous scatter of `activeRecordingFileHandle`,
/// `activeRecordingFileURL`, `pendingAudioFileURL`, and a free-floating
/// `recordingIOQueue` across AppDelegate — collapsing the file lifecycle to a
/// single typed reference so it's harder to accidentally delete-too-early
/// (data loss) or forget-to-delete (disk leak).
///
/// Two construction modes:
///   - `init?(creatingAt:)` opens a fresh file + write handle for live capture
///   - `init(capturedAt:)` wraps an existing file (recovered from a prior
///     session's crash) — no write handle, can only be deleted
final class RecordingArtifact {
    let url: URL
    private var liveHandle: FileHandle?
    private let ioQueue: DispatchQueue

    /// Live-recording mode. Returns nil if the file can't be created (disk
    /// full, perms broken) — caller proceeds without a disk snapshot for
    /// that one dictation. The in-memory packets path remains the source of
    /// truth for transcription either way.
    init?(creatingAt url: URL) {
        guard FileManager.default.createFile(atPath: url.path, contents: nil),
              let handle = FileHandle(forWritingAtPath: url.path) else {
            try? FileManager.default.removeItem(at: url)
            return nil
        }
        self.url = url
        self.liveHandle = handle
        self.ioQueue = DispatchQueue(label: "com.wisprlightning.recording.io")
    }

    /// Captured-file mode for recovery: file exists on disk, no live writes.
    init(capturedAt url: URL) {
        self.url = url
        self.liveHandle = nil
        // Captured artifacts never call append(); the queue is here just so
        // delete() can use the same sync-drain pattern uniformly.
        self.ioQueue = DispatchQueue(label: "com.wisprlightning.recording.io")
    }

    /// Append a PCM packet asynchronously. No-op if the write handle has
    /// been closed (post-stop) or torn down by an earlier write error.
    func append(_ packet: Data) {
        ioQueue.async { [weak self] in
            guard let self = self, let handle = self.liveHandle else { return }
            do {
                try handle.write(contentsOf: packet)
            } catch {
                // Disk error — drop the handle so subsequent writes don't
                // keep retrying against a dead descriptor. The in-memory
                // packets path is unaffected.
                NSLog("Wispr Lightning: incremental audio write failed: %@", error.localizedDescription)
                try? handle.close()
                self.liveHandle = nil
            }
        }
    }

    /// Drain queued writes and close the write handle. Idempotent. The file
    /// stays on disk so the transcription pipeline can hand it to recovery
    /// later if needed.
    func finishWriting() {
        ioQueue.sync {
            try? self.liveHandle?.close()
            self.liveHandle = nil
        }
    }

    /// Delete the on-disk file. Closes the write handle first. Idempotent.
    func delete() {
        finishWriting()
        try? FileManager.default.removeItem(at: url)
        wLog("Deleted saved audio: \(url.lastPathComponent)")
    }

    /// Schedule deletion after `delay` seconds. Used by the polish path —
    /// polish runs async and might hang; we want a grace window where the
    /// .pcm survives in case recovery has to step in.
    ///
    /// Strong capture is deliberate: the caller's last reference to the
    /// artifact often goes out of scope immediately after this call (e.g.
    /// the success-path local `artifactToRetire` falls off the stack), and
    /// `pendingAudio` has already been nilled by the time we get here. A
    /// `[weak self]` capture would let the artifact deallocate and silently
    /// skip the delete — the file would then linger until the 24h sweep.
    func deleteAfter(_ delay: TimeInterval) {
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + delay) {
            self.delete()
        }
    }
}
