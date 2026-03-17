import AppKit

class MusicController {
    private let settings: AppSettings
    private let lock = NSLock()
    private var musicWasPlaying = false
    private var spotifyWasPlaying = false

    init(settings: AppSettings) {
        self.settings = settings
    }

    func pauseMusic() {
        guard settings.muteMusic else { return }

        let group = DispatchGroup()

        // Check and pause Apple Music and Spotify in parallel
        if isAppRunning("com.apple.Music") {
            group.enter()
            DispatchQueue.global(qos: .userInitiated).async {
                let paused = self.runAppleScript(
                    "tell application \"Music\" to if player state is playing then\npause\nreturn \"paused\"\nend if"
                ) == "paused"
                self.lock.lock()
                self.musicWasPlaying = paused
                self.lock.unlock()
                group.leave()
            }
        }

        if isAppRunning("com.spotify.client") {
            group.enter()
            DispatchQueue.global(qos: .userInitiated).async {
                let paused = self.runAppleScript(
                    "tell application \"Spotify\" to if player state is playing then\npause\nreturn \"paused\"\nend if"
                ) == "paused"
                self.lock.lock()
                self.spotifyWasPlaying = paused
                self.lock.unlock()
                group.leave()
            }
        }

        group.wait()
    }

    func resumeMusic() {
        guard settings.muteMusic else { return }

        lock.lock()
        let resumeMusic = musicWasPlaying
        let resumeSpotify = spotifyWasPlaying
        musicWasPlaying = false
        spotifyWasPlaying = false
        lock.unlock()

        if resumeMusic {
            _ = runAppleScript("tell application \"Music\" to play")
        }
        if resumeSpotify {
            _ = runAppleScript("tell application \"Spotify\" to play")
        }
    }

    private func isAppRunning(_ bundleIdentifier: String) -> Bool {
        NSWorkspace.shared.runningApplications.contains { $0.bundleIdentifier == bundleIdentifier }
    }

    private func runAppleScript(_ source: String) -> String? {
        let script = NSAppleScript(source: source)
        var error: NSDictionary?
        let result = script?.executeAndReturnError(&error)
        return result?.stringValue
    }
}
