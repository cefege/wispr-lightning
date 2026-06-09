import AppKit

class StatusBarController {
    private let statusItem: NSStatusItem
    private let session: Session
    private let settings: AppSettings
    private let historyStore: HistoryStore
    private let dictionaryStore: DictionaryStore
    private let notesStore: NotesStore
    private let textInjector: TextInjector
    private var settingsWindowController: SettingsWindowController?
    private var lastTranscription: String?
    private var sessionObserver: NSObjectProtocol?
    /// Polls TCC permissions every 30s while the app is alive so a mid-session
    /// revocation (user opens Privacy & Security and toggles Accessibility
    /// off) flips the menu warning instead of waiting for the next launch.
    private var permissionPollTimer: Timer?
    private var lastPermissionSnapshot: [Permission: PermissionStatus] = [:]
    /// Cache of recent crash reports. Scanning ~/Library/Logs/DiagnosticReports
    /// is filesystem I/O — buildMenu runs on every settings/session change,
    /// so calling Self.recentCrashReports synchronously each time was hitting
    /// disk dozens of times per session. Refresh lazily once per 5 minutes
    /// (and once on launch) — crash reports don't arrive faster than that.
    private var cachedCrashReports: [URL] = []
    private var crashReportsCachedAt: Date = .distantPast

    /// Wired by AppDelegate to flip HotkeyListener's pause state.
    var onTogglePause: (() -> Void)?
    /// Wired by AppDelegate to re-open the permissions wizard.
    var onShowOnboarding: (() -> Void)?

    init(session: Session, settings: AppSettings, historyStore: HistoryStore, dictionaryStore: DictionaryStore, notesStore: NotesStore, textInjector: TextInjector) {
        self.session = session
        self.settings = settings
        self.historyStore = historyStore
        self.dictionaryStore = dictionaryStore
        self.notesStore = notesStore
        self.textInjector = textInjector
        self.statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)

        if let button = statusItem.button {
            button.image = Self.menuBarIcon(accessibilityDescription: "Wispr Lightning")
        }

        // Load last transcription from history
        if let latest = historyStore.getEntries().first {
            lastTranscription = latest.formattedText ?? latest.asrText
        }

        buildMenu()
        refreshPermissionSnapshot()

        sessionObserver = NotificationCenter.default.addObserver(
            forName: .sessionChanged,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.buildMenu()
        }

        // Low-rate TCC poll so a mid-session revocation doesn't go unnoticed.
        permissionPollTimer = Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in
            self?.checkPermissionDrift()
        }
    }

    deinit {
        if let observer = sessionObserver {
            NotificationCenter.default.removeObserver(observer)
        }
        permissionPollTimer?.invalidate()
        permissionPollTimer = nil
    }

    private func refreshPermissionSnapshot() {
        var snap: [Permission: PermissionStatus] = [:]
        for p in Permission.allCases { snap[p] = PermissionsManager.status(p) }
        lastPermissionSnapshot = snap
    }

    private func checkPermissionDrift() {
        var next: [Permission: PermissionStatus] = [:]
        for p in Permission.allCases { next[p] = PermissionsManager.status(p) }
        if next != lastPermissionSnapshot {
            lastPermissionSnapshot = next
            buildMenu()
        }
    }

    /// True when any required permission has flipped away from `.granted`.
    private var hasPermissionRegression: Bool {
        for p in Permission.allCases where p.isRequired {
            if lastPermissionSnapshot[p] != .granted { return true }
        }
        return false
    }

    func setLastTranscription(_ text: String) {
        lastTranscription = text
        buildMenu()
    }

    func updateMenu() {
        buildMenu()
    }

    func setRecording(_ recording: Bool) {
        isRecordingIcon = recording
        refreshStatusIcon()
    }

    private var isRecordingIcon = false

    /// Refresh the menu-bar icon based on whether there's a recording in
    /// flight AND whether any alert (auth missing / permission revoked) is
    /// active. Without the alert overlay, an OpenRouter user whose Flow
    /// session expired (and Flow is in their fallback chain) would have to
    /// open the menu to discover the problem.
    private func refreshStatusIcon() {
        guard let button = statusItem.button else { return }
        let needsAttention = alertReasonForStatusIcon() != nil
        button.image = Self.menuBarIcon(
            accessibilityDescription: isRecordingIcon ? "Recording" : "Wispr Lightning",
            tintWithAttention: needsAttention
        )
    }

    /// Same set of conditions buildMenu uses to pin the orange / red items
    /// at the top of the menu. When non-nil, the icon gets the attention
    /// overlay so the user notices without opening the menu.
    private func alertReasonForStatusIcon() -> String? {
        let chainVendors: [String] = [settings.activeVendor] + settings.fallbackChain.map { $0.vendor }
        if chainVendors.contains(DictationVendor.wisprFlow.rawValue) && !session.isValid {
            return "Wispr Flow sign-in required"
        }
        if hasPermissionRegression {
            return "A required permission was revoked"
        }
        return nil
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

    private static func menuBarIcon(accessibilityDescription: String,
                                    tintWithAttention: Bool = false) -> NSImage? {
        if tintWithAttention {
            // Composite a small exclamation badge in the upper-right corner
            // of the base icon. Built once and cached. The "attention" base
            // icon is reused for every refresh.
            if let badged = cachedAttentionIcon {
                badged.accessibilityDescription = accessibilityDescription + " — needs attention"
                return badged
            }
            return cachedMenuBarIcon
        }
        cachedMenuBarIcon?.accessibilityDescription = accessibilityDescription
        return cachedMenuBarIcon
    }

    /// Icon with a small exclamation badge overlaid. Decoded once.
    private static let cachedAttentionIcon: NSImage? = {
        guard let base = cachedMenuBarIcon else { return nil }
        let result = NSImage(size: base.size)
        result.lockFocus()
        base.draw(in: NSRect(origin: .zero, size: base.size))
        // Orange badge in the corner.
        let badgeSide: CGFloat = max(7, base.size.width * 0.42)
        let badgeRect = NSRect(
            x: base.size.width - badgeSide,
            y: base.size.height - badgeSide,
            width: badgeSide, height: badgeSide
        )
        NSColor.systemOrange.setFill()
        NSBezierPath(ovalIn: badgeRect).fill()
        result.unlockFocus()
        result.isTemplate = false
        return result
    }()

    private func buildMenu() {
        let menu = NSMenu()

        // Auth alerts — pinned to the top so the user notices before they
        // press the hotkey and dictate into the void. Trigger when Flow is
        // configured anywhere in the chain (primary OR fallback), not just
        // primary — otherwise a chain like OpenRouter→Flow would silently
        // hit a dead Flow step and the user would never know to re-sign-in.
        let chainVendors: [String] = [settings.activeVendor] + settings.fallbackChain.map { $0.vendor }
        let flowInChain = chainVendors.contains(DictationVendor.wisprFlow.rawValue)
        if flowInChain && !session.isValid {
            let item = NSMenuItem(title: "⚠ Wispr Flow sign-in required",
                                  action: #selector(openSettingsWindow), keyEquivalent: "")
            item.target = self
            let attrs: [NSAttributedString.Key: Any] = [.foregroundColor: NSColor.systemOrange]
            item.attributedTitle = NSAttributedString(string: "⚠ Wispr Flow sign-in required", attributes: attrs)
            menu.addItem(item)
            menu.addItem(NSMenuItem.separator())
        }

        if hasPermissionRegression {
            let item = NSMenuItem(title: "⚠ A required permission was revoked",
                                  action: #selector(showOnboardingWindow), keyEquivalent: "")
            item.target = self
            let attrs: [NSAttributedString.Key: Any] = [.foregroundColor: NSColor.systemRed]
            item.attributedTitle = NSAttributedString(string: "⚠ A required permission was revoked", attributes: attrs)
            menu.addItem(item)
            menu.addItem(NSMenuItem.separator())
        }

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

        // Undo last dictation — posts Cmd+Z to the focused app.
        // Disabled when there's nothing to undo; cleared after firing so we don't over-undo
        // into the user's prior text on a second press.
        let undoItem = NSMenuItem(title: "Undo last dictation", action: #selector(undoLastDictation), keyEquivalent: "")
        undoItem.target = self
        undoItem.isEnabled = !(lastTranscription?.isEmpty ?? true)
        menu.addItem(undoItem)

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

        // Transcription provider submenu — quick switch between Wispr Flow,
        // OpenRouter, and Claude Voice without opening Settings.
        let providerItem = NSMenuItem(title: "Provider", action: nil, keyEquivalent: "")
        let providerMenu = NSMenu()
        let active = DictationVendor(rawValue: settings.activeVendor) ?? .wisprFlow
        for vendor in DictationVendor.allCases {
            let item = NSMenuItem(title: vendor.displayName, action: #selector(selectVendor(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = vendor.rawValue
            item.state = vendor == active ? .on : .off
            providerMenu.addItem(item)
        }
        providerItem.submenu = providerMenu
        menu.addItem(providerItem)

        // Pause hotkey toggle — escape hatch for Universal Control / remote desktop
        // scenarios where the hotkey shouldn't fire on this Mac.
        let pauseTitle = settings.hotkeyPaused ? "Resume hotkey" : "Pause hotkey"
        let pauseItem = NSMenuItem(title: pauseTitle, action: #selector(togglePauseHotkey), keyEquivalent: "")
        pauseItem.target = self
        if settings.hotkeyPaused { pauseItem.state = .on }
        menu.addItem(pauseItem)

        let naturalItem = NSMenuItem(title: "Natural Mode", action: #selector(toggleNaturalMode), keyEquivalent: "")
        naturalItem.target = self
        naturalItem.state = settings.naturalModeEnabled ? .on : .off
        menu.addItem(naturalItem)

        let setupItem = NSMenuItem(title: "Setup & Permissions…", action: #selector(showOnboardingWindow), keyEquivalent: "")
        setupItem.target = self
        menu.addItem(setupItem)

        let settingsItem = NSMenuItem(title: "Settings", action: #selector(openSettingsWindow), keyEquivalent: ",")
        settingsItem.keyEquivalentModifierMask = .command
        settingsItem.target = self
        menu.addItem(settingsItem)

        menu.addItem(NSMenuItem.separator())

        // Recent crash report — show one menu item per new .ips file under
        // ~/Library/Logs/DiagnosticReports/. Clicking reveals it in Finder
        // so the user can drag it into a bug report. No leading separator
        // here — the previous block already added one above.
        let crashes = cachedCrashReportsIfFresh()
        if !crashes.isEmpty {
            for url in crashes.prefix(2) {
                let item = NSMenuItem(title: "🐞 Reveal crash report (\(url.lastPathComponent))",
                                      action: #selector(revealCrashReport(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = url
                menu.addItem(item)
            }
        }

        // Build / version footer — pulled from Info.plist (set by install.sh).
        menu.addItem(NSMenuItem.separator())
        let version = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "dev"
        let versionItem = NSMenuItem(title: "Wispr Lightning v\(version)", action: nil, keyEquivalent: "")
        versionItem.isEnabled = false
        let font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        versionItem.attributedTitle = NSAttributedString(
            string: "Wispr Lightning v\(version)",
            attributes: [.font: font, .foregroundColor: NSColor.tertiaryLabelColor]
        )
        menu.addItem(versionItem)

        let quitItem = NSMenuItem(title: "Quit Wispr Lightning", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "")
        menu.addItem(quitItem)

        self.statusItem.menu = menu
        refreshStatusIcon()
    }

    /// Return crash reports created since this launch's start time (well,
    /// within the last 7 days as a heuristic) — newest first.
    private static func recentCrashReports() -> [URL]? {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Logs/DiagnosticReports")
        guard let files = try? FileManager.default.contentsOfDirectory(
            at: dir, includingPropertiesForKeys: [.creationDateKey]
        ) else { return nil }
        let recent = files.filter { url in
            // Crash report files are named `<binary>-<timestamp>.ips` or
            // `<binary>_<timestamp>-<uuid>.crash`. Match strictly on the
            // hyphen / underscore boundary so `WisprLightningExtension-*.ips`
            // (hypothetical future) doesn't match.
            let name = url.lastPathComponent
            let isOurs = name.hasPrefix("WisprLightning-") || name.hasPrefix("WisprLightning_")
            let isCrash = url.pathExtension == "ips" || url.pathExtension == "crash"
            return isOurs && isCrash
        }.sorted { lhs, rhs in
            let l = (try? lhs.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            let r = (try? rhs.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            return l > r
        }.filter { url in
            guard let c = try? url.resourceValues(forKeys: [.creationDateKey]).creationDate else { return false }
            return Date().timeIntervalSince(c) < 7 * 86400
        }
        return recent.isEmpty ? nil : recent
    }

    @objc private func revealCrashReport(_ sender: NSMenuItem) {
        guard let url = sender.representedObject as? URL else { return }
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }

    /// Returns the cached crash-report list; kicks a background refresh if
    /// the cache is older than 5 minutes. Returning the stale list is fine —
    /// a brand-new crash report just shows up after the next buildMenu, not
    /// instantly. The benefit is buildMenu no longer hits disk every call.
    private func cachedCrashReportsIfFresh() -> [URL] {
        if Date().timeIntervalSince(crashReportsCachedAt) > 300 {
            crashReportsCachedAt = Date()
            DispatchQueue.global(qos: .utility).async { [weak self] in
                let fresh = Self.recentCrashReports() ?? []
                DispatchQueue.main.async {
                    guard let self else { return }
                    if fresh.map(\.absoluteString) != self.cachedCrashReports.map(\.absoluteString) {
                        self.cachedCrashReports = fresh
                        self.buildMenu()
                    }
                }
            }
        }
        return cachedCrashReports
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

    @objc private func showOnboardingWindow() {
        onShowOnboarding?()
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

    @objc private func selectVendor(_ sender: NSMenuItem) {
        guard let raw = sender.representedObject as? String,
              let vendor = DictationVendor(rawValue: raw) else { return }
        settings.activeVendor = vendor.rawValue
        settings.save()
        buildMenu()
    }

    @objc private func togglePauseHotkey() {
        onTogglePause?()
        buildMenu()
    }

    @objc private func toggleNaturalMode() {
        settings.naturalModeEnabled.toggle()
        settings.save()
        buildMenu()
    }

    @objc private func copyLastTranscription() {
        guard let text = lastTranscription, !text.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    @objc private func undoLastDictation() {
        guard let text = lastTranscription, !text.isEmpty else { return }
        textInjector.undoLastInjection()
        // One-shot: clear so menu disables itself. Pressing undo twice would over-undo
        // into whatever the user had before the dictation.
        lastTranscription = nil
        buildMenu()
        wLog("Undo last dictation — \(text.count) chars")
    }
}
