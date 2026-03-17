import AppKit

class StatusBarController {
    private let statusItem: NSStatusItem
    private let session: Session
    private let settings: AppSettings
    private let historyStore: HistoryStore
    private let dictionaryStore: DictionaryStore
    private let notesStore: NotesStore
    private var mainWindow: MainWindow?
    private var settingsWindowController: SettingsWindowController?
    private var lastTranscription: String?

    init(session: Session, settings: AppSettings, historyStore: HistoryStore, dictionaryStore: DictionaryStore, notesStore: NotesStore) {
        self.session = session
        self.settings = settings
        self.historyStore = historyStore
        self.dictionaryStore = dictionaryStore
        self.notesStore = notesStore
        self.statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)

        if let button = statusItem.button {
            button.image = NSImage(systemSymbolName: "bolt.fill", accessibilityDescription: "Wispr Lightning")
            button.image?.isTemplate = true
        }

        // Load last transcription from history
        if let latest = historyStore.getEntries().first {
            lastTranscription = latest.formattedText ?? latest.asrText
        }

        buildMenu()

        NotificationCenter.default.addObserver(
            forName: .sessionChanged,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.buildMenu()
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
            if recording {
                button.image = NSImage(systemSymbolName: "bolt.horizontal.fill", accessibilityDescription: "Recording")
                button.image?.isTemplate = true
            } else {
                button.image = NSImage(systemSymbolName: "bolt.fill", accessibilityDescription: "Wispr Lightning")
                button.image?.isTemplate = true
            }
        }
    }

    private func buildMenu() {
        let menu = NSMenu()

        // Status line with hotkey hint
        let hotkeyLabel = settings.hotkeyLabel
        let statusText: String
        if session.isValid {
            statusText = "⚡ Ready — hold \(hotkeyLabel) to dictate"
        } else {
            statusText = "⚠️ Not signed in"
        }
        let statusItem = NSMenuItem(title: statusText, action: nil, keyEquivalent: "")
        statusItem.isEnabled = false
        let statusFont = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize, weight: .medium)
        statusItem.attributedTitle = NSAttributedString(string: statusText, attributes: [.font: statusFont])
        menu.addItem(statusItem)

        menu.addItem(NSMenuItem.separator())

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

        let openItem = NSMenuItem(title: "Open Wispr Lightning...", action: #selector(openMainWindow), keyEquivalent: "")
        openItem.target = self
        menu.addItem(openItem)

        menu.addItem(NSMenuItem.separator())

        let settingsItem = NSMenuItem(title: "Settings...", action: #selector(openSettingsWindow), keyEquivalent: ",")
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
            settingsWindowController = SettingsWindowController(settings: settings, session: session)
        }
        settingsWindowController?.showWindow()
    }

    @objc private func openSettingsWindow() {
        openSettings()
    }

    @objc func openMainWindow() {
        if mainWindow == nil {
            mainWindow = MainWindow(session: session, settings: settings, historyStore: historyStore, dictionaryStore: dictionaryStore, notesStore: notesStore)
        }
        mainWindow?.showWindow()
    }

    @objc private func copyLastTranscription() {
        guard let text = lastTranscription, !text.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}
