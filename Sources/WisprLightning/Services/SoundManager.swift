import AppKit

class SoundManager {
    private let settings: AppSettings

    init(settings: AppSettings) {
        self.settings = settings
    }

    func playStart() {
        guard settings.enableSounds else { return }
        NSSound(named: "Tink")?.play()
    }

    func playStop() {
        guard settings.enableSounds else { return }
        NSSound(named: "Pop")?.play()
    }
}
