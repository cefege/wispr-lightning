import Foundation

class AppSettings: Codable {
    var hotkeyKeyCode: UInt16 = 59       // Left Ctrl (legacy single-key)
    var hotkeyLabel: String = "Left Control"  // legacy single-key
    var hotkeyKeyCodes: [UInt16] = [59]
    var hotkeyLabels: [String] = ["Left Control"]
    var micDeviceUID: String? = nil       // nil = system default
    var micDeviceName: String? = nil
    var languages: [String] = ["en"]
    var launchAtLogin: Bool = false
    var showInDock: Bool = false
    var enableSounds: Bool = true
    var muteMusic: Bool = false

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
}
