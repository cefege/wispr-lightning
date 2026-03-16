import AppKit

class StatusBarController {
    private let statusItem: NSStatusItem
    private let session: Session
    private let settings: AppSettings
    private let historyStore: HistoryStore
    private var mainWindow: MainWindow?
    private var lastTranscription: String?

    init(session: Session, settings: AppSettings, historyStore: HistoryStore) {
        self.session = session
        self.settings = settings
        self.historyStore = historyStore
        self.statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)

        if let button = statusItem.button {
            button.image = NSImage(systemSymbolName: "mic.fill", accessibilityDescription: "Wispr Lite")
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
                button.image = NSImage(systemSymbolName: "waveform", accessibilityDescription: "Recording")
                button.image?.isTemplate = true
            } else {
                button.image = NSImage(systemSymbolName: "mic.fill", accessibilityDescription: "Wispr Lite")
                button.image?.isTemplate = true
            }
        }
    }

    private func buildMenu() {
        let menu = NSMenu()

        // Last transcription preview
        if let text = lastTranscription, !text.isEmpty {
            let preview = text.count > 60 ? String(text.prefix(60)) + "…" : text
            let previewItem = NSMenuItem(title: preview, action: #selector(copyLastTranscription), keyEquivalent: "")
            previewItem.target = self
            let font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
            let attributes: [NSAttributedString.Key: Any] = [.font: font]
            previewItem.attributedTitle = NSAttributedString(string: preview, attributes: attributes)
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

        let openItem = NSMenuItem(title: "Open Wispr Lite...", action: #selector(openMainWindow), keyEquivalent: "")
        openItem.target = self
        menu.addItem(openItem)

        menu.addItem(NSMenuItem.separator())

        let quitItem = NSMenuItem(title: "Quit Wispr Lite", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "")
        menu.addItem(quitItem)

        statusItem.menu = menu
    }

    @objc private func openMainWindow() {
        if mainWindow == nil {
            mainWindow = MainWindow(session: session, settings: settings, historyStore: historyStore)
        }
        mainWindow?.showWindow()
    }

    @objc private func copyLastTranscription() {
        guard let text = lastTranscription, !text.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}
