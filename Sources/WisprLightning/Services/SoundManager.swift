import AppKit
import AVFoundation

class SoundManager {
    private let settings: AppSettings
    private var startPlayer: AVAudioPlayer?
    private var stopPlayer: AVAudioPlayer?
    private var pastePlayer: AVAudioPlayer?

    init(settings: AppSettings) {
        self.settings = settings
        loadSoundPack()

        NotificationCenter.default.addObserver(
            forName: .settingsChanged, object: nil, queue: .main
        ) { [weak self] _ in
            self?.loadSoundPack()
        }

        NotificationCenter.default.addObserver(
            forName: .previewSoundPack, object: nil, queue: .main
        ) { [weak self] _ in
            self?.playStart()
        }
    }

    func loadSoundPack() {
        let packName = settings.selectedSoundPack ?? "default"

        if let startURL = soundURL(name: "dictation-start", pack: packName) {
            startPlayer = try? AVAudioPlayer(contentsOf: startURL)
            startPlayer?.prepareToPlay()
        } else {
            // Fallback to system sounds
            startPlayer = nil
        }

        if let stopURL = soundURL(name: "dictation-stop", pack: packName) {
            stopPlayer = try? AVAudioPlayer(contentsOf: stopURL)
            stopPlayer?.prepareToPlay()
        } else {
            stopPlayer = nil
        }

        if let pasteURL = soundURL(name: "paste", pack: packName) {
            pastePlayer = try? AVAudioPlayer(contentsOf: pasteURL)
            pastePlayer?.prepareToPlay()
        }
    }

    private func soundURL(name: String, pack: String) -> URL? {
        // Look in bundle resources
        if let url = Bundle.main.url(forResource: name, withExtension: "wav", subdirectory: "Sounds/\(pack)") {
            return url
        }
        // Fallback to default pack
        if pack != "default", let url = Bundle.main.url(forResource: name, withExtension: "wav", subdirectory: "Sounds/default") {
            return url
        }
        return nil
    }

    func playStart() {
        guard settings.enableSounds else { return }
        if let player = startPlayer {
            player.currentTime = 0
            player.play()
        } else {
            NSSound(named: "Tink")?.play()
        }
    }

    func playStop() {
        guard settings.enableSounds else { return }
        if let player = stopPlayer {
            player.currentTime = 0
            player.play()
        } else {
            NSSound(named: "Pop")?.play()
        }
    }

    func playPaste() {
        guard settings.enableSounds else { return }
        pastePlayer?.currentTime = 0
        pastePlayer?.play()
    }

    static func availablePacks() -> [String] {
        guard let soundsURL = Bundle.main.url(forResource: "Sounds", withExtension: nil) else {
            return ["default"]
        }
        let contents = (try? FileManager.default.contentsOfDirectory(at: soundsURL, includingPropertiesForKeys: nil)) ?? []
        let packs = contents.filter { $0.hasDirectoryPath }.map { $0.lastPathComponent }.sorted()
        return packs.isEmpty ? ["default"] : packs
    }
}
