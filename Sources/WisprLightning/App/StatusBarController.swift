import AppKit

class StatusBarController {
    private let statusItem: NSStatusItem
    private let session: Session
    private let settings: AppSettings
    private let historyStore: HistoryStore
    private let dictionaryStore: DictionaryStore
    private let notesStore: NotesStore
    private var settingsWindowController: SettingsWindowController?
    private var lastTranscription: String?
    private var sessionObserver: NSObjectProtocol?

    /// Wired by AppDelegate to flip HotkeyListener's pause state.
    var onTogglePause: (() -> Void)?

    init(session: Session, settings: AppSettings, historyStore: HistoryStore, dictionaryStore: DictionaryStore, notesStore: NotesStore) {
        self.session = session
        self.settings = settings
        self.historyStore = historyStore
        self.dictionaryStore = dictionaryStore
        self.notesStore = notesStore
        self.statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)

        if let button = statusItem.button {
            button.image = Self.menuBarIcon(accessibilityDescription: "Wispr Lightning")
        }

        // Load last transcription from history
        if let latest = historyStore.getEntries().first {
            lastTranscription = latest.formattedText ?? latest.asrText
        }

        buildMenu()

        sessionObserver = NotificationCenter.default.addObserver(
            forName: .sessionChanged,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.buildMenu()
        }
    }

    deinit {
        if let observer = sessionObserver {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    func setLastTranscription(_ text: String) {
        lastTranscription = text
        buildMenu()
    }

    func updateMenu() {
        buildMenu()
    }

    func setRecording(_ recording: Bool) {
        if let button = statusItem.button {
            button.image = Self.menuBarIcon(
                accessibilityDescription: recording ? "Recording" : "Wispr Lightning"
            )
        }
    }

    /// Cached menu-bar icon — decoded once at first access. Wispr Flow brand
    /// PNG (not a template) when available, system mic symbol as fallback.
    private static let cachedMenuBarIcon: NSImage? = {
        if let path = Bundle.main.path(forResource: "WisprFlowIcon", ofType: "png"),
           let img = NSImage(contentsOfFile: path) {
            img.size = NSSize(width: 18, height: 18)
            img.isTemplate = false
            return img
        }
        let fallback = NSImage(systemSymbolName: "mic.fill", accessibilityDescription: nil)
        fallback?.isTemplate = true
        return fallback
    }()

    private static func menuBarIcon(accessibilityDescription: String) -> NSImage? {
        cachedMenuBarIcon?.accessibilityDescription = accessibilityDescription
        return cachedMenuBarIcon
    }

    private func buildMenu() {
        let menu = NSMenu()

        // Last transcription preview
        if let text = lastTranscription, !text.isEmpty {
            let preview = text.count > 60 ? String(text.prefix(60)) + "…" : text
            let previewItem = NSMenuItem(title: preview, action: #selector(copyLastTranscription), keyEquivalent: "")
            previewItem.target = self
            let font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
            previewItem.attributedTitle = NSAttributedString(string: preview, attributes: [.font: font])
            menu.addItem(previewItem)
        } else {
            let emptyItem = NSMenuItem(title: "No recent dictation", action: nil, keyEquivalent: "")
            emptyItem.isEnabled = false
            let font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
            let attributes: [NSAttributedString.Key: Any] = [
                .font: font,
                .foregroundColor: NSColor.secondaryLabelColor
            ]
            emptyItem.attributedTitle = NSAttributedString(string: "No recent dictation", attributes: attributes)
            menu.addItem(emptyItem)
        }

        menu.addItem(NSMenuItem.separator())

        // Input Device submenu
        let inputDeviceItem = NSMenuItem(title: "Input Device", action: nil, keyEquivalent: "")
        let inputDeviceMenu = NSMenu()

        let defaultItem = NSMenuItem(title: "System Default", action: #selector(selectMicDevice(_:)), keyEquivalent: "")
        defaultItem.target = self
        defaultItem.state = settings.micDeviceUID == nil ? .on : .off
        inputDeviceMenu.addItem(defaultItem)

        let devices = AudioRecorder.listInputDevices()
        if !devices.isEmpty {
            inputDeviceMenu.addItem(NSMenuItem.separator())
            for device in devices {
                let item = NSMenuItem(title: device.name, action: #selector(selectMicDevice(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = device.uid
                item.state = settings.micDeviceUID == device.uid ? .on : .off
                inputDeviceMenu.addItem(item)
            }
        }

        inputDeviceItem.submenu = inputDeviceMenu
        menu.addItem(inputDeviceItem)

        // Pause hotkey toggle — escape hatch for Universal Control / remote desktop
        // scenarios where the hotkey shouldn't fire on this Mac.
        let pauseTitle = settings.hotkeyPaused ? "Resume hotkey" : "Pause hotkey"
        let pauseItem = NSMenuItem(title: pauseTitle, action: #selector(togglePauseHotkey), keyEquivalent: "")
        pauseItem.target = self
        if settings.hotkeyPaused { pauseItem.state = .on }
        menu.addItem(pauseItem)

        let settingsItem = NSMenuItem(title: "Settings", action: #selector(openSettingsWindow), keyEquivalent: ",")
        settingsItem.keyEquivalentModifierMask = .command
        settingsItem.target = self
        menu.addItem(settingsItem)

        menu.addItem(NSMenuItem.separator())

        let quitItem = NSMenuItem(title: "Quit Wispr Lightning", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "")
        menu.addItem(quitItem)

        self.statusItem.menu = menu
    }

    func openSettings() {
        if settingsWindowController == nil {
            settingsWindowController = SettingsWindowController(settings: settings, session: session, historyStore: historyStore, dictionaryStore: dictionaryStore, notesStore: notesStore)
        }
        settingsWindowController?.showWindow()
    }

    @objc private func openSettingsWindow() {
        openSettings()
    }

    @objc private func selectMicDevice(_ sender: NSMenuItem) {
        if let uid = sender.representedObject as? String {
            settings.micDeviceUID = uid
            settings.micDeviceName = sender.title
        } else {
            settings.micDeviceUID = nil
            settings.micDeviceName = nil
        }
        settings.save()
        buildMenu()
    }

    @objc private func togglePauseHotkey() {
        onTogglePause?()
        buildMenu()
    }

    @objc private func copyLastTranscription() {
        guard let text = lastTranscription, !text.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}
