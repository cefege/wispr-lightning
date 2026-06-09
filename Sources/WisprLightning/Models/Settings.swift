import Foundation

/// One step in the user-configured fallback chain. `vendor` is a
/// `DictationVendor` rawValue; `openRouterModel` is honoured only when
/// `vendor == openRouter` and lets the chain include multiple OpenRouter
/// models with different speed/quality tradeoffs.
struct FallbackStep: Codable, Hashable, Identifiable {
    var id: UUID
    var vendor: String
    var openRouterModel: String?

    init(vendor: String, openRouterModel: String? = nil) {
        self.id = UUID()
        self.vendor = vendor
        self.openRouterModel = openRouterModel
    }
}

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
    /// When true, a quick press from idle enters hands-free locked recording
    /// directly; a second press stops it. Holding the key still works as PTT
    /// (release → stop) so existing muscle memory keeps working.
    /// Kept as a stored bool for backward compat with old settings.json; new
    /// code reads `hotkeyPressBehavior` and writes it instead.
    var hotkeyTapToToggle: Bool = false

    /// Authoritative press behavior. "hold" = push-to-talk only, "toggle" =
    /// tap-to-start + tap-to-stop (quick release locks recording immediately),
    /// "legacy" = hold-or-double-tap-to-lock (original Lightning behavior).
    var hotkeyPressBehavior: String = "legacy"

    // Natural Mode — type text character-by-character instead of pasting
    var naturalModeEnabled: Bool = false
    var naturalModeSpeed: String = "normal"  // "slow" | "normal" | "expert"

    // Transcription vendor — see DictationVendor enum
    var activeVendor: String = DictationVendor.wisprFlow.rawValue
    var openRouterModel: String = "google/gemini-2.5-flash-lite"
    /// Ordered fallback chain. When the primary vendor fails with a hard
    /// error (auth / connection / server / timeout), Lightning rebuilds the
    /// dictation provider as `fallbackChain[0]`, retries with the same audio,
    /// and walks the chain on subsequent failures.
    var fallbackChain: [FallbackStep] = []

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

    static var backupURL: URL { settingsURL.appendingPathExtension("bak") }

    static func load() -> AppSettings {
        // Primary path; fall back to .bak if main file is missing or corrupt.
        if let data = try? Data(contentsOf: settingsURL),
           let settings = try? JSONDecoder().decode(AppSettings.self, from: data) {
            // Snapshot a backup of the just-validated file so a future
            // corruption can recover the last-known-good state.
            try? FileManager.default.removeItem(at: backupURL)
            try? FileManager.default.copyItem(at: settingsURL, to: backupURL)
            return applyMigrations(settings)
        }
        if let data = try? Data(contentsOf: backupURL),
           let settings = try? JSONDecoder().decode(AppSettings.self, from: data) {
            NSLog("Wispr Lightning: settings.json was unreadable; restored from .bak")
            return applyMigrations(settings)
        }
        let settings = AppSettings()
        settings.save()
        return settings
    }

    private static func applyMigrations(_ settings: AppSettings) -> AppSettings {
        // One-time migration: older settings files only carried the legacy
        // single-key fields. Seed the array form from them so the rest of the
        // app (which only reads the array) sees the user's previous binding.
        if settings.hotkeyKeyCodes.isEmpty && settings.hotkeyKeyCode != 0 {
            settings.hotkeyKeyCodes = [settings.hotkeyKeyCode]
            settings.hotkeyLabels = [settings.hotkeyLabel]
        }
        // One-time migration from the old hotkeyTapToToggle bool. Existing
        // users who had it on stay on the toggle mode; everyone else stays on
        // the legacy hold-or-double-tap-to-lock behavior they were used to.
        if settings.hotkeyPressBehavior.isEmpty {
            settings.hotkeyPressBehavior = settings.hotkeyTapToToggle ? "toggle" : "legacy"
        }
        return settings
    }

    /// Serial queue for save serialization. JSONEncoder + pretty-print +
    /// atomic file write on every settings change adds up — bouncing them
    /// here keeps the main thread responsive while preserving last-write-
    /// wins ordering.
    /// Stored on the type rather than the instance so synthesised Codable
    /// conformance doesn't trip over a non-Codable DispatchWorkItem.
    private static let saveQueue = DispatchQueue(label: "com.wisprlightning.settings.save")
    private static let pendingSaveLock = NSLock()
    private static var pendingSaveItem: DispatchWorkItem?

    func save() {
        // Post the changed notification on the main thread immediately — UI
        // observers shouldn't wait for the disk write to redraw.
        let postNotification: () -> Void = { [weak self] in
            guard let self else { return }
            NotificationCenter.default.post(name: .settingsChanged, object: self)
        }
        if Thread.isMainThread { postNotification() }
        else { DispatchQueue.main.async { postNotification() } }

        // Debounce rapid changes — only the last save in a 100ms window hits
        // disk. Settings toggled in rapid succession (e.g. picker scrolling)
        // coalesce.
        let snapshot = self.encodedSnapshot()
        Self.pendingSaveLock.lock()
        Self.pendingSaveItem?.cancel()
        let item = DispatchWorkItem {
            guard let data = snapshot else { return }
            try? data.write(to: Self.settingsURL, options: .atomic)
        }
        Self.pendingSaveItem = item
        Self.pendingSaveLock.unlock()
        Self.saveQueue.asyncAfter(deadline: .now() + 0.1, execute: item)
    }

    /// Snapshot the current state on the calling thread (almost always main)
    /// so the deferred disk write doesn't read mutable state from a queue.
    private func encodedSnapshot() -> Data? {
        guard let data = try? JSONEncoder().encode(self) else { return nil }
        if let json = try? JSONSerialization.jsonObject(with: data),
           let pretty = try? JSONSerialization.data(withJSONObject: json, options: .prettyPrinted) {
            return pretty
        }
        return data
    }
}

extension Notification.Name {
    static let settingsChanged = Notification.Name("WisprLightningSettingsChanged")
    static let sessionChanged = Notification.Name("WisprSessionChanged")
    static let previewSoundPack = Notification.Name("WisprPreviewSoundPack")
    static let audioDevicesChanged = Notification.Name("WisprAudioDevicesChanged")
}
