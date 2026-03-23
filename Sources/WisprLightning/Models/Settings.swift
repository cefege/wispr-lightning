import Foundation

class AppSettings: Codable {
    var hotkeyKeyCode: UInt16 = 59       // Left Ctrl (legacy single-key)
    var hotkeyLabel: String = "Left Control"  // legacy single-key
    var hotkeyKeyCodes: [UInt16] = [59]
    var hotkeyLabels: [String] = ["Left Control"]
    var micDeviceUID: String? = nil       // nil = system default
    var micDeviceName: String? = nil
    var keepMicrophoneActive: Bool = false
    var languages: [String] = ["en"]
    var launchAtLogin: Bool = false
    var showInDock: Bool = false
    var enableSounds: Bool = true
    var muteMusic: Bool = false
    var aiFormatting: Bool = true
    var autoCleanupLevel: String = "light"
    var commandModeEnabled: Bool = true
    var useScreenContext: Bool = false
    var useAccessibilityContext: Bool = true
    var shareUsageData: Bool = false
    var styleDetectionEnabled: Bool = true
    var personalizationStyles: [String: String] = ["work": "default", "email": "default", "personal": "default", "other": "default"]
    var hyperlinkOn: Bool = false
    var autoLearnWords: Bool = true

    // Polish
    var polishEnabled: Bool = false
    var polishInstructions: [String: Bool] = [
        "Make more concise": true,
        "Reword for clarity": true,
        "Maintain your tone": true,
        "Reorder for readability": true,
        "Add structure for readability": true,
        "Clarify main point": false,
        "Refine phrasing for impact": false
    ]
    var activePolishInstructions: [String] {
        polishInstructions.filter { $0.value }.map { $0.key }
    }
    var autoPolish: Bool = false
    var polishHotkeyKeyCodes: [UInt16] = [62]  // Right Control
    var polishHotkeyLabels: [String] = ["Right Control"]

    // Email Signatures
    var emailAutoSignature: Bool = false
    var emailSignatureOption: String = "written_with_lightning"

    // Creator Mode
    var creatorMode: Bool = false

    // Sound Packs
    var selectedSoundPack: String? = nil

    // Debug
    var verboseLogging: Bool = false

    static let settingsURL: URL = {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/WisprLightning")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("settings.json")
    }()

    static func load() -> AppSettings {
        guard FileManager.default.fileExists(atPath: settingsURL.path),
              let data = try? Data(contentsOf: settingsURL),
              let settings = try? JSONDecoder().decode(AppSettings.self, from: data) else {
            let settings = AppSettings()
            settings.save()
            return settings
        }
        return settings
    }

    func save() {
        guard let data = try? JSONEncoder().encode(self) else { return }
        // Pretty print
        if let json = try? JSONSerialization.jsonObject(with: data),
           let pretty = try? JSONSerialization.data(withJSONObject: json, options: .prettyPrinted) {
            try? pretty.write(to: Self.settingsURL)
        } else {
            try? data.write(to: Self.settingsURL)
        }
        NotificationCenter.default.post(name: .settingsChanged, object: self)
    }
}

extension Notification.Name {
    static let settingsChanged = Notification.Name("WisprLightningSettingsChanged")
    static let sessionChanged = Notification.Name("WisprSessionChanged")
    static let previewSoundPack = Notification.Name("WisprPreviewSoundPack")
}
