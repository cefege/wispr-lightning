import AppKit

class MusicController {
    private let settings: AppSettings
    private var musicWasPlaying = false
    private var spotifyWasPlaying = false

    init(settings: AppSettings) {
        self.settings = settings
    }

    func pauseMusic() {
        guard settings.muteMusic else { return }

        // Check and pause Apple Music (only if running)
        if isAppRunning("com.apple.Music") {
            musicWasPlaying = runAppleScript("tell application \"Music\" to player state is playing") == "true"
            if musicWasPlaying {
                _ = runAppleScript("tell application \"Music\" to pause")
            }
        }

        // Check and pause Spotify (only if running)
        if isAppRunning("com.spotify.client") {
            spotifyWasPlaying = runAppleScript("tell application \"Spotify\" to player state is playing") == "true"
            if spotifyWasPlaying {
                _ = runAppleScript("tell application \"Spotify\" to pause")
            }
        }
    }

    func resumeMusic() {
        guard settings.muteMusic else { return }

        if musicWasPlaying {
            _ = runAppleScript("tell application \"Music\" to play")
            musicWasPlaying = false
        }
        if spotifyWasPlaying {
            _ = runAppleScript("tell application \"Spotify\" to play")
            spotifyWasPlaying = false
        }
    }

    private func isAppRunning(_ bundleIdentifier: String) -> Bool {
        !NSWorkspace.shared.runningApplications.filter { $0.bundleIdentifier == bundleIdentifier }.isEmpty
    }

    private func runAppleScript(_ source: String) -> String? {
        let script = NSAppleScript(source: source)
        var error: NSDictionary?
        let result = script?.executeAndReturnError(&error)
        return result?.stringValue
    }
}
