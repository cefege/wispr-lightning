import Foundation

class AppSettings: Codable {
    // Deprecated — kept for Codable backward-compat. All readers use the array form.
    var hotkeyKeyCode: UInt16 = 59
    var hotkeyLabel: String = "Left Control"
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

    // Hotkey
    var hotkeyPaused: Bool = false

    // Natural Mode — type text character-by-character instead of pasting
    var naturalModeEnabled: Bool = false
    var naturalModeSpeed: String = "normal"  // "slow" | "normal" | "expert"

    // Transcription vendor — see DictationVendor enum
    var activeVendor: String = DictationVendor.wisprFlow.rawValue
    var openRouterModel: String = "google/gemini-2.5-flash-lite"

    // Onboarding — flipped to true once the user has closed the wizard at
    // least once. The wizard still auto-shows on subsequent launches if any
    // required permission is missing.
    var didCompleteOnboarding: Bool = false

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
        // One-time migration: older settings files only carried the legacy
        // single-key fields. Seed the array form from them so the rest of the
        // app (which only reads the array) sees the user's previous binding.
        if settings.hotkeyKeyCodes.isEmpty && settings.hotkeyKeyCode != 0 {
            settings.hotkeyKeyCodes = [settings.hotkeyKeyCode]
            settings.hotkeyLabels = [settings.hotkeyLabel]
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
    static let audioDevicesChanged = Notification.Name("WisprAudioDevicesChanged")
}
