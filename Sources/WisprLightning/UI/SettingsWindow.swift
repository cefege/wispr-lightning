import AppKit
import SwiftUI

private extension View {
    @ViewBuilder func removeSidebarToggleIfAvailable() -> some View {
        if #available(macOS 14.0, *) {
            self.toolbar(removing: .sidebarToggle)
        } else {
            self
        }
    }
}

// MARK: - Settings Toggle Row

private struct SettingsToggleRow: View {
    let title: String
    let description: String?
    @Binding var isOn: Bool

    init(_ title: String, description: String? = nil, isOn: Binding<Bool>) {
        self.title = title
        self.description = description
        self._isOn = isOn
    }

    var body: some View {
        Toggle(isOn: $isOn) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                if let desc = description {
                    Text(desc)
                        .font(.subheadline)
                        .fontWeight(.regular)
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .toggleStyle(.switch)
        .controlSize(.small)
    }
}

// MARK: - Settings Section Enum

enum SettingsSection: String, CaseIterable, Identifiable {
    case general, dictation, accounts, provider, polish
    case history, dictionary, notes
    case privacy, system

    var id: String { rawValue }

    var title: String {
        switch self {
        case .general: return "General"
        case .dictation: return "Dictation"
        case .accounts: return "Accounts"
        case .provider: return "Provider"
        case .polish: return "Polish"
        case .history: return "History"
        case .dictionary: return "Dictionary"
        case .notes: return "Notes"
        case .privacy: return "Privacy"
        case .system: return "System"
        }
    }

    var icon: String {
        switch self {
        case .general: return "gearshape.fill"
        case .dictation: return "mic.fill"
        case .accounts: return "person.crop.circle.fill"
        case .provider: return "antenna.radiowaves.left.and.right"
        case .polish: return "sparkles"
        case .history: return "clock.fill"
        case .dictionary: return "character.book.closed.fill"
        case .notes: return "note.text"
        case .privacy: return "hand.raised.fill"
        case .system: return "desktopcomputer"
        }
    }

    var iconGradient: LinearGradient {
        switch self {
        case .general, .system:    return Self.gradGray
        case .dictation, .privacy: return Self.gradBlue
        case .accounts:            return Self.gradBlue
        case .provider:            return Self.gradGreen
        case .polish:              return Self.gradPurple
        case .history:             return Self.gradOrange
        case .dictionary:          return Self.gradGreen
        case .notes:               return Self.gradYellow
        }
    }

    private static func grad(_ t: Color, _ b: Color) -> LinearGradient {
        LinearGradient(colors: [t, b], startPoint: .top, endPoint: .bottom)
    }
    private static let gradGray   = grad(Color(red:0.64,green:0.64,blue:0.70), Color(red:0.48,green:0.48,blue:0.55))
    private static let gradBlue   = grad(Color(red:0.30,green:0.57,blue:1.00), Color(red:0.14,green:0.38,blue:0.96))
    private static let gradPurple = grad(Color(red:0.72,green:0.38,blue:1.00), Color(red:0.55,green:0.22,blue:0.94))
    private static let gradOrange = grad(Color(red:1.00,green:0.68,blue:0.22), Color(red:0.98,green:0.50,blue:0.02))
    private static let gradGreen  = grad(Color(red:0.34,green:0.82,blue:0.44), Color(red:0.20,green:0.70,blue:0.30))
    private static let gradYellow = grad(Color(red:1.00,green:0.84,blue:0.18), Color(red:0.98,green:0.70,blue:0.04))
}

// Colored icon tile matching macOS System Settings style
private struct SectionIcon: View {
    let section: SettingsSection
    var body: some View {
        Image(systemName: section.icon)
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: 28, height: 28)
            .background(section.iconGradient, in: RoundedRectangle(cornerRadius: 7))
    }
}

// MARK: - All Settings View (sidebar + detail)

struct AllSettingsView: View {
    private static let sidebarIcon: NSImage? = {
        guard let path = Bundle.main.path(forResource: "WisprFlowIcon", ofType: "png") else { return nil }
        return NSImage(contentsOfFile: path)
    }()

    @ObservedObject var vm: SettingsViewModel
    let session: Session
    @StateObject private var historyVM: HistoryViewModel
    @StateObject private var dictionaryVM: DictionaryViewModel
    @StateObject private var notesVM: NotesViewModel
    @State private var selectedSection: SettingsSection = .general

    init(vm: SettingsViewModel, session: Session, historyStore: HistoryStore, dictionaryStore: DictionaryStore, notesStore: NotesStore) {
        self.vm = vm
        self.session = session
        self._historyVM = StateObject(wrappedValue: HistoryViewModel(historyStore: historyStore))
        self._dictionaryVM = StateObject(wrappedValue: DictionaryViewModel(dictionaryStore: dictionaryStore))
        self._notesVM = StateObject(wrappedValue: NotesViewModel(notesStore: notesStore))
    }

    private static let dataGroup: [SettingsSection] = [.history, .dictionary, .notes]
    private static let systemGroup: [SettingsSection] = [.privacy, .system]

    /// Polish is a Wispr Flow-only feature; hide the tab entirely for other vendors.
    private var settingsGroup: [SettingsSection] {
        let vendor = DictationVendor(rawValue: vm.activeVendor) ?? .wisprFlow
        return session.canUsePolish(activeVendor: vendor)
            ? [.general, .dictation, .accounts, .provider, .polish]
            : [.general, .dictation, .accounts, .provider]
    }

    var body: some View {
        NavigationSplitView {
            List(selection: $selectedSection) {
                Section {
                    ForEach(settingsGroup) { section in
                        sidebarRow(section)
                    }
                }
                Section {
                    ForEach(Self.dataGroup) { section in
                        sidebarRow(section)
                    }
                }
                Section {
                    ForEach(Self.systemGroup) { section in
                        sidebarRow(section)
                    }
                }
            }
            .listStyle(.sidebar)
            .safeAreaInset(edge: .top, spacing: 0) {
                if let nsImage = Self.sidebarIcon {
                    HStack {
                        Spacer()
                        Image(nsImage: nsImage)
                            .resizable()
                            .frame(width: 64, height: 64)
                            .clipShape(RoundedRectangle(cornerRadius: 14))
                        Spacer()
                    }
                    .padding(.top, 16)
                    .padding(.bottom, 8)
                    .background(.clear)
                }
            }
            .navigationSplitViewColumnWidth(220)
        } detail: {
            Group {
                switch selectedSection {
                case .history:
                    HistoryView(vm: historyVM)
                case .dictionary:
                    DictionaryView(vm: dictionaryVM)
                case .notes:
                    NotesView(vm: notesVM)
                default:
                    ScrollView {
                        VStack(alignment: .leading, spacing: Theme.Spacing.large) {
                            switch selectedSection {
                            case .general:
                                ShortcutsDetail(vm: vm)
                                Divider()
                                MicrophoneDetail(vm: vm)
                                Divider()
                                LanguagesDetail(vm: vm)
                            case .dictation:
                                DictationDetail(vm: vm)
                                Divider()
                                PersonalizationDetail(vm: vm)
                            case .accounts:
                                AccountsDetail(vm: vm, session: session)
                            case .provider:
                                ProviderDetail(vm: vm, session: session)
                            case .polish:
                                PolishDetail(vm: vm)
                            case .privacy:
                                PrivacyDetail(vm: vm)
                            case .system:
                                SystemDetail(vm: vm)
                            default:
                                EmptyView()
                            }
                        }
                        .padding(28)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            .navigationTitle(selectedSection.title)
        }
        .removeSidebarToggleIfAvailable()
    }

    @ViewBuilder
    private func sidebarRow(_ section: SettingsSection) -> some View {
        Label {
            Text(section.title)
        } icon: {
            SectionIcon(section: section)
        }
        .tag(section)
        .padding(.vertical, 1)
    }
}

// MARK: - Settings Window Controller

class SettingsWindowController: NSObject, NSWindowDelegate {
    private var window: NSWindow?
    private var settingsVM: SettingsViewModel?
    private let settings: AppSettings
    private let session: Session
    private let historyStore: HistoryStore
    private let dictionaryStore: DictionaryStore
    private let notesStore: NotesStore
    /// Raises Settings whenever the app becomes active. Without this, after
    /// a Keychain prompt or any modal dialog steals focus, macOS hands focus
    /// back to whatever was frontmost before Lightning — not to our window —
    /// and an LSUIElement app with no dock icon feels like it "hides".
    private var becomeActiveObserver: NSObjectProtocol?

    init(settings: AppSettings, session: Session, historyStore: HistoryStore, dictionaryStore: DictionaryStore, notesStore: NotesStore) {
        self.settings = settings
        self.session = session
        self.historyStore = historyStore
        self.dictionaryStore = dictionaryStore
        self.notesStore = notesStore
    }

    deinit {
        if let observer = becomeActiveObserver {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    /// Tracks the policy we had before opening Settings so windowWillClose
    /// can restore it (don't promote to `.regular` permanently if the user
    /// hadn't checked "Show in Dock").
    private var policyBeforeOpen: NSApplication.ActivationPolicy?

    private func promoteToRegular() {
        if policyBeforeOpen == nil {
            policyBeforeOpen = NSApp.activationPolicy()
        }
        // .regular = dock icon + cmd-tab + proper focus behavior. Without this,
        // an LSUIElement app loses focus to whatever was previously frontmost
        // every time a Keychain / system dialog steals focus from us.
        if NSApp.activationPolicy() != .regular {
            NSApp.setActivationPolicy(.regular)
        }
    }

    private func restorePolicy() {
        if let prior = policyBeforeOpen, prior != .regular {
            NSApp.setActivationPolicy(prior)
        }
        policyBeforeOpen = nil
    }

    func showWindow() {
        promoteToRegular()
        if let window = window {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            startObservingActivation()
            return
        }

        let svm = SettingsViewModel(settings: settings)
        self.settingsVM = svm

        let settingsView = AllSettingsView(vm: svm, session: session, historyStore: historyStore, dictionaryStore: dictionaryStore, notesStore: notesStore)
        let hostingView = NSHostingView(rootView: settingsView)

        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 860, height: 580),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        w.title = "Wispr Lightning Settings"
        w.titlebarAppearsTransparent = false
        w.toolbarStyle = .unified
        w.titleVisibility = .visible
        w.center()
        w.isReleasedWhenClosed = false
        w.minSize = NSSize(width: 680, height: 460)
        w.contentView = hostingView
        w.setFrameAutosaveName("SettingsWindow")
        // Hide the window from the cmd-h / cmd-w "everything closes" sweep so
        // a stray hotkey doesn't lose the user's place mid-setup.
        w.collectionBehavior = [.fullScreenAuxiliary, .moveToActiveSpace]
        w.delegate = self

        self.window = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        startObservingActivation()
    }

    // MARK: - Re-raise on activation

    private func startObservingActivation() {
        guard becomeActiveObserver == nil else { return }
        becomeActiveObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.window?.makeKeyAndOrderFront(nil)
        }
    }

    private func stopObservingActivation() {
        if let observer = becomeActiveObserver {
            NotificationCenter.default.removeObserver(observer)
            becomeActiveObserver = nil
        }
    }

    // NSWindowDelegate — when the user closes the Settings window, stop
    // re-raising it on every app activation and revert the activation policy
    // so the app goes back to being a menu-bar accessory (if it was before).
    func windowWillClose(_ notification: Notification) {
        stopObservingActivation()
        restorePolicy()
    }
}

// MARK: - Shortcuts Detail

private struct ShortcutsDetail: View {
    @ObservedObject var vm: SettingsViewModel

    private func pressBehaviorHint(_ value: String) -> String {
        switch value {
        case "hold":
            return "Recording lasts as long as the key is held. Releasing always ends it."
        case "toggle":
            return "Press once to start, press again to stop. Holding still works as push-to-talk."
        default:
            return "Quick tap waits for a second tap to lock hands-free. Hold longer than ~0.5s for push-to-talk."
        }
    }

    var body: some View {
        Text("Dictation Hotkeys")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
                Text("Any of these keys will start dictation:")
                    .foregroundStyle(.secondary)

                ForEach(Array(vm.hotkeyLabels.enumerated()), id: \.offset) { index, label in
                    HStack(spacing: Theme.Spacing.medium) {
                        KeyCapView(label: label)

                        if vm.hotkeyLabels.count > 1 {
                            Button {
                                vm.removeHotkey(at: index)
                            } label: {
                                Image(systemName: "minus.circle")
                                    .foregroundStyle(.red)
                            }
                            .buttonStyle(.borderless)
                            .help("Remove this hotkey")
                        }
                    }
                }

                Button(vm.isCapturingShortcut ? "Press a key…" : "Add Hotkey") {
                    vm.startCapturing()
                }
                .controlSize(.small)

                Text("Modifier keys work as hold-to-talk. Regular keys use press-to-toggle.")
                    .font(.subheadline)
                    .foregroundStyle(.tertiary)

                Divider()

                HotkeyConflictTester(expectedKeyCodes: vm.hotkeyKeyCodesSet)

                Text("Some hotkeys are claimed by macOS or other apps (e.g. Fn opens dictation; ⌥-space is Spotlight on some configs). If your hotkey is intercepted, Lightning won't see the press — pick something else.")
                    .font(.caption)
                    .foregroundStyle(.tertiary)

                Divider()

                VStack(alignment: .leading, spacing: 6) {
                    Text("Press behavior").font(.subheadline.weight(.medium))
                    Picker("Press behavior", selection: $vm.hotkeyPressBehavior) {
                        Text("Hold to talk").tag("hold")
                        Text("Tap to start, tap to stop").tag("toggle")
                        Text("Hold or double-tap to lock (legacy)").tag("legacy")
                    }
                    .labelsHidden()
                    .pickerStyle(.radioGroup)
                    .onChange(of: vm.hotkeyPressBehavior) { _ in vm.saveHotkeyPressBehavior() }

                    Text(pressBehaviorHint(vm.hotkeyPressBehavior))
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(Theme.Spacing.medium)
        }
    }
}

/// Lightweight live key listener inside Settings. When the user focuses the
/// "Test your hotkey" prompt, an NSEvent local monitor watches the next
/// flagsChanged / keyDown event and flashes feedback. If the configured
/// hotkey is intercepted by another app or the OS, the user sees the
/// prompt stay quiet and knows to pick something else.
private struct HotkeyConflictTester: View {
    let expectedKeyCodes: Set<UInt16>
    @State private var lastSeen: String? = nil
    @State private var matchedAt: Date? = nil
    @State private var monitor: Any? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Test your hotkey")
                .font(.subheadline.weight(.medium))
            HStack(spacing: 10) {
                ZStack {
                    Capsule()
                        .fill(matched ? Color.green.opacity(0.18) : Color.secondary.opacity(0.10))
                        .frame(width: 220, height: 28)
                    Text(label)
                        .font(.caption.monospaced())
                        .foregroundStyle(matched ? .green : .secondary)
                }
                if matched {
                    Image(systemName: "checkmark.seal.fill")
                        .foregroundStyle(.green)
                }
            }
        }
        .onAppear { install() }
        .onDisappear { uninstall() }
    }

    private var matched: Bool {
        guard let ts = matchedAt else { return false }
        return Date().timeIntervalSince(ts) < 1.5
    }

    private var label: String {
        if let seen = lastSeen { return seen }
        return "Press your hotkey to confirm Lightning sees it…"
    }

    private func install() {
        uninstall()
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.flagsChanged, .keyDown]) { event in
            let code = event.keyCode
            let name = HotkeyListener.keycodeLabels[code] ?? "Key \(code)"
            if expectedKeyCodes.contains(code) {
                DispatchQueue.main.async {
                    lastSeen = "Detected: \(name)"
                    matchedAt = Date()
                }
            } else {
                DispatchQueue.main.async {
                    lastSeen = "Saw \(name) (not your bound hotkey)"
                }
            }
            return event
        }
    }

    private func uninstall() {
        if let m = monitor {
            NSEvent.removeMonitor(m)
            monitor = nil
        }
    }
}

// MARK: - Microphone Detail

private struct MicrophoneDetail: View {
    @ObservedObject var vm: SettingsViewModel

    var body: some View {
        Text("Input Device")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
                Picker("Microphone", selection: $vm.selectedMicUID) {
                    Text("System Default").tag(String?.none)
                    ForEach(vm.micDevices, id: \.uid) { device in
                        Text(device.name).tag(Optional(device.uid))
                    }
                }
                .labelsHidden()
                .onChange(of: vm.selectedMicUID) { _ in vm.saveMicSelection() }

                Button {
                    vm.refreshMicDevices()
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .controlSize(.small)

                Divider()

                SettingsToggleRow("Keep microphone active",
                    description: "Eliminates startup delay — recommended when using iPhone as microphone",
                    isOn: $vm.keepMicrophoneActive)
                    .onChange(of: vm.keepMicrophoneActive) { _ in vm.saveMicSelection() }
            }
            .padding(Theme.Spacing.medium)
        }
    }
}

// MARK: - Languages Detail

private struct LanguagesDetail: View {
    @ObservedObject var vm: SettingsViewModel
    @State private var searchText = ""

    private var filteredLanguages: [SettingsViewModel.Language] {
        if searchText.isEmpty {
            return SettingsViewModel.languages
        }
        let query = searchText.lowercased()
        return SettingsViewModel.languages.filter { $0.name.lowercased().contains(query) }
    }

    private var selectedLanguages: [SettingsViewModel.Language] {
        SettingsViewModel.languages.filter { vm.selectedLanguages.contains($0.code) }
    }

    var body: some View {
        Text("Dictation Languages")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.small) {
                // Auto-detect toggle
                SettingsToggleRow("Auto-detect",
                    description: "Automatically detect the spoken language",
                    isOn: Binding(
                        get: { vm.isAutoDetect },
                        set: { _ in vm.toggleLanguage(SettingsViewModel.autoDetectCode) }
                    ))
                .fontWeight(.medium)
                .padding(.bottom, Theme.Spacing.small)

                Divider()
                    .padding(.vertical, Theme.Spacing.small)

                if !vm.isAutoDetect {
                    // Selected languages as removable chips
                    if !selectedLanguages.isEmpty {
                        FlowLayout(spacing: 6) {
                            ForEach(selectedLanguages, id: \.code) { lang in
                                HStack(spacing: 4) {
                                    Text("\(lang.flag) \(lang.name)")
                                        .font(.subheadline)
                                    Button {
                                        vm.toggleLanguage(lang.code)
                                    } label: {
                                        Image(systemName: "xmark.circle.fill")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                    .buttonStyle(.borderless)
                                }
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(Color.accentColor.opacity(0.12))
                                .cornerRadius(12)
                            }
                        }
                        .padding(.bottom, Theme.Spacing.small)
                    }

                    // Search field
                    TextField("Search languages...", text: $searchText)
                        .textFieldStyle(.roundedBorder)
                        .padding(.bottom, Theme.Spacing.small)

                    // Scrollable language list
                    ZStack(alignment: .bottom) {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 0) {
                                ForEach(filteredLanguages, id: \.code) { lang in
                                    Toggle(isOn: Binding(
                                        get: { vm.selectedLanguages.contains(lang.code) },
                                        set: { _ in vm.toggleLanguage(lang.code) }
                                    )) {
                                        Text("\(lang.flag) \(lang.name)")
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                    }
                                    .toggleStyle(.switch)
                                    .controlSize(.small)
                                    .font(.body)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 5)
                                    Divider()
                                        .padding(.leading, 8)
                                }
                            }
                            .padding(.vertical, 4)
                        }
                        .scrollIndicators(.visible)
                        .frame(height: 220)
                        .background(Color(nsColor: .textBackgroundColor))
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                        .overlay(
                            RoundedRectangle(cornerRadius: 6)
                                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
                        )

                        // Fade hint indicating more content below
                        LinearGradient(
                            colors: [.clear, Color(nsColor: .textBackgroundColor).opacity(0.85)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                        .frame(height: 28)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                        .allowsHitTesting(false)
                    }
                } else {
                    Text("All supported languages will be recognized automatically. Specifying languages manually can improve accuracy.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(Theme.Spacing.medium)
        }
    }
}

// MARK: - Flow Layout for Language Chips

private struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? .infinity
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > maxWidth && x > 0 {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }

        return CGSize(width: maxWidth, height: y + rowHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x: CGFloat = bounds.minX
        var y: CGFloat = bounds.minY
        var rowHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > bounds.maxX && x > bounds.minX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), proposal: .unspecified)
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}

// MARK: - Dictation Detail

private struct DictationDetail: View {
    @ObservedObject var vm: SettingsViewModel

    var body: some View {
        Text("Dictation")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
                SettingsToggleRow("AI Formatting",
                    description: "Apply AI formatting to clean up transcriptions",
                    isOn: $vm.aiFormatting)
                    .onChange(of: vm.aiFormatting) { _ in vm.saveDictationSettings() }

                Picker("Cleanup Level", selection: $vm.autoCleanupLevel) {
                    ForEach(SettingsViewModel.cleanupLevels, id: \.value) { level in
                        Text(level.label).tag(level.value)
                    }
                }
                .pickerStyle(.segmented)
                .onChange(of: vm.autoCleanupLevel) { _ in vm.saveDictationSettings() }
                Text("How aggressively to clean up filler words")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                SettingsToggleRow("Voice Commands",
                    description: "Interpret phrases like \"new line\" as commands",
                    isOn: $vm.commandModeEnabled)
                    .onChange(of: vm.commandModeEnabled) { _ in vm.saveDictationSettings() }

                SettingsToggleRow("Auto-detect hyperlinks",
                    description: "Convert spoken URLs to clickable hyperlinks",
                    isOn: $vm.hyperlinkOn)
                    .onChange(of: vm.hyperlinkOn) { _ in vm.saveDictationSettings() }

                SettingsToggleRow("Auto-learn words",
                    description: "Automatically learn new vocabulary from dictations",
                    isOn: $vm.autoLearnWords)
                    .onChange(of: vm.autoLearnWords) { _ in vm.saveDictationSettings() }

                Divider()

                SettingsToggleRow("Email signature",
                    description: "Append a signature when dictating in email apps",
                    isOn: $vm.emailAutoSignature)
                    .onChange(of: vm.emailAutoSignature) { _ in vm.saveDictationSettings() }

                if vm.emailAutoSignature {
                    Picker("Signature", selection: $vm.emailSignatureOption) {
                        Text("Written with Wispr Lightning").tag("written_with_lightning")
                        Text("Spoken with Wispr Lightning").tag("spoken_with_lightning")
                    }
                    .pickerStyle(.menu)
                    .onChange(of: vm.emailSignatureOption) { _ in vm.saveDictationSettings() }
                }

                Divider()

                SettingsToggleRow("Creator mode",
                    description: "Extended recording for long-form content (up to 10 min)",
                    isOn: $vm.creatorMode)
                    .onChange(of: vm.creatorMode) { _ in vm.saveDictationSettings() }

                Divider()

                SettingsToggleRow("Natural Mode",
                    description: "Type text character-by-character instead of pasting (slower but feels human)",
                    isOn: $vm.naturalModeEnabled)
                    .onChange(of: vm.naturalModeEnabled) { _ in vm.saveDictationSettings() }

                if vm.naturalModeEnabled {
                    Picker("Typing speed", selection: $vm.naturalModeSpeed) {
                        Text("Slow").tag("slow")
                        Text("Normal").tag("normal")
                        Text("Expert").tag("expert")
                    }
                    .pickerStyle(.segmented)
                    .onChange(of: vm.naturalModeSpeed) { _ in vm.saveDictationSettings() }
                    Text("Slow ≈ 30 WPM, Normal ≈ 50 WPM, Expert ≈ 80 WPM")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(Theme.Spacing.medium)
        }
    }
}

// MARK: - Provider Detail

private struct ProviderDetail: View {
    @ObservedObject var vm: SettingsViewModel
    let session: Session


    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.large) {
            Text("Transcription Chain")
                .font(.title3.weight(.semibold))

            Text("Step 1 is your primary provider. If it fails with a hard error (auth, network, server, timeout), Lightning automatically retries the same audio against step 2, then step 3, and so on. Empty transcripts don't fall through. Set up vendor credentials in the Accounts tab.")
                .font(.callout)
                .foregroundColor(.secondary)

            primaryRow

            ForEach(Array(vm.fallbackChain.enumerated()), id: \.element.id) { index, step in
                FallbackStepRow(
                    index: index,
                    step: step,
                    session: session,
                    models: vm.openRouterModelList,
                    modelListState: vm.openRouterModelListState,
                    onChangeVendor: { newVendor in
                        vm.updateFallbackStepVendor(at: index, vendor: newVendor)
                    },
                    onChangeModel: { newModel in
                        vm.updateFallbackStepModel(at: index, model: newModel)
                    },
                    onRemove: { vm.removeFallbackStep(at: index) },
                    onMoveUp: {
                        if index == 0 {
                            vm.promoteToPrimary(at: 0)
                        } else {
                            vm.moveFallbackStep(from: index, to: index - 1)
                        }
                    },
                    onMoveDown: index < vm.fallbackChain.count - 1 ? { vm.moveFallbackStep(from: index, to: index + 2) } : nil
                )
            }

            Button("+ Add fallback") { vm.addFallbackStep() }
                .controlSize(.small)
        }
        .onAppear { vm.loadOpenRouterModels() }
    }

    /// Step 1 row — same layout as FallbackStepRow but tied to settings.activeVendor
    /// and settings.openRouterModel, with only a Move-down button (you can't
    /// remove the primary, and there's nothing above it to move into).
    @ViewBuilder
    private var primaryRow: some View {
        HStack(alignment: .top, spacing: 10) {
            Text("1.")
                .font(.body.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 26, alignment: .trailing)

            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Picker("Vendor", selection: $vm.activeVendor) {
                        ForEach(DictationVendor.allCases, id: \.rawValue) { vendor in
                            Text(vendor.displayName).tag(vendor.rawValue)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: 280, alignment: .leading)
                    .onChange(of: vm.activeVendor) { _ in vm.saveActiveVendor() }

                    VendorReadinessBadge(
                        vendor: DictationVendor(rawValue: vm.activeVendor) ?? .wisprFlow,
                        session: session
                    )
                }

                if vm.activeVendor == DictationVendor.openRouter.rawValue {
                    Picker("Model", selection: $vm.openRouterModel) {
                        if case .loaded = vm.openRouterModelListState {
                            ForEach(vm.openRouterModelList) { m in
                                Text(m.displayLabel).tag(m.id)
                            }
                            if !vm.openRouterModelList.contains(where: { $0.id == vm.openRouterModel }) {
                                Text("Custom: \(vm.openRouterModel)").tag(vm.openRouterModel)
                            }
                        } else {
                            Text("Loading models…").tag("loading-placeholder").disabled(true)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: 420, alignment: .leading)
                    .onChange(of: vm.openRouterModel) { _ in vm.saveProviderSettings() }
                }
            }

            Spacer()

            Button { vm.demotePrimary() } label: {
                Image(systemName: "chevron.down")
            }
            .buttonStyle(.borderless)
            .help(vm.fallbackChain.isEmpty
                  ? "Move primary down (appends a new fallback step)"
                  : "Move primary down — swap with the first fallback")
        }
        .padding(10)
        .background(Color(NSColor.controlBackgroundColor))
        .cornerRadius(8)
    }

}

/// Small warning chip next to a vendor picker when that vendor lacks the
/// credentials it needs to actually run a dictation. Renders nothing when
/// the vendor is ready, so callers can splat `VendorReadinessBadge(...)`
/// unconditionally into a layout.
struct VendorReadinessBadge: View {
    let vendor: DictationVendor
    let session: Session

    static func make(vendor: DictationVendor, session: Session) -> VendorReadinessBadge {
        return VendorReadinessBadge(vendor: vendor, session: session)
    }

    var body: some View {
        if !vendor.isReady(session: session) {
            Label("Not signed in", systemImage: "exclamationmark.triangle.fill")
                .font(.caption2)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(Color.orange.opacity(0.18), in: Capsule())
                .foregroundStyle(.orange)
                .help("Set up this vendor in the Accounts tab.")
        }
    }
}

private struct FallbackStepRow: View {
    let index: Int
    let step: FallbackStep
    let session: Session
    let models: [OpenRouterAudioModel]
    let modelListState: SettingsViewModel.OpenRouterModelListState
    let onChangeVendor: (String) -> Void
    let onChangeModel: (String?) -> Void
    let onRemove: () -> Void
    let onMoveUp: (() -> Void)?
    let onMoveDown: (() -> Void)?

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Text("\(index + 2).")  // 1 is the primary; chain starts at 2
                .font(.body.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: 26, alignment: .trailing)

            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Picker("Vendor", selection: Binding(
                        get: { step.vendor },
                        set: { onChangeVendor($0) }
                    )) {
                        ForEach(DictationVendor.allCases, id: \.rawValue) { v in
                            Text(v.displayName).tag(v.rawValue)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: 280, alignment: .leading)

                    VendorReadinessBadge(
                        vendor: DictationVendor(rawValue: step.vendor) ?? .wisprFlow,
                        session: session
                    )
                }

                if step.vendor == DictationVendor.openRouter.rawValue {
                    Picker("Model", selection: Binding(
                        get: { step.openRouterModel ?? "" },
                        set: { onChangeModel($0.isEmpty ? nil : $0) }
                    )) {
                        Text("Use primary OpenRouter model").tag("")
                        if case .loaded = modelListState {
                            ForEach(models) { m in
                                Text(m.displayLabel).tag(m.id)
                            }
                            if let chosen = step.openRouterModel,
                               !models.contains(where: { $0.id == chosen }) {
                                Text("Custom: \(chosen)").tag(chosen)
                            }
                        } else {
                            Text("Loading models…").tag("loading-placeholder").disabled(true)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: 420, alignment: .leading)
                }
            }

            Spacer()

            HStack(spacing: 4) {
                if let onMoveUp {
                    Button { onMoveUp() } label: {
                        Image(systemName: "chevron.up")
                    }
                    .buttonStyle(.borderless)
                    .help("Move up")
                }
                if let onMoveDown {
                    Button { onMoveDown() } label: {
                        Image(systemName: "chevron.down")
                    }
                    .buttonStyle(.borderless)
                    .help("Move down")
                }
                Button { onRemove() } label: {
                    Image(systemName: "minus.circle")
                        .foregroundStyle(.red)
                }
                .buttonStyle(.borderless)
                .help("Remove this fallback step")
            }
        }
        .padding(10)
        .background(Color(NSColor.controlBackgroundColor))
        .cornerRadius(8)
    }
}

// MARK: - Accounts Detail

/// One tab for all per-vendor credentials. Each vendor has its own card with
/// its own sign-in / API-key / Keychain-check affordance. Setting up auth
/// here is separate from arranging the chain in the Provider tab — letting
/// the user prep credentials once, then mix and match in any order.
private struct AccountsDetail: View {
    @ObservedObject var vm: SettingsViewModel
    let session: Session

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.large) {
            Text("Accounts")
                .font(.title3.weight(.semibold))

            Text("Set up sign-in or API keys for each vendor here. Use the Provider tab to choose which one is active and arrange the fallback chain.")
                .font(.callout)
                .foregroundColor(.secondary)

            vendorCard(title: DictationVendor.wisprFlow.displayName) {
                WisprFlowAccountPanel(session: session)
            }

            vendorCard(title: DictationVendor.openRouter.displayName) {
                OpenRouterAccountPanel(vm: vm)
            }

            vendorCard(title: DictationVendor.claudeVoice.displayName) {
                ClaudeVoiceAuthRow()
            }
        }
        .onAppear { vm.loadOpenRouterModels() }
    }

    @ViewBuilder
    private func vendorCard<Content: View>(title: String, @ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
            Text(title)
                .font(.headline)
            content()
        }
        .padding(Theme.Spacing.medium)
        .background(Color(NSColor.controlBackgroundColor))
        .cornerRadius(10)
    }
}

/// OpenRouter API key + Test connection. Lives in the Accounts tab; the
/// per-step model picker lives next to its row in the Provider chain.
private struct OpenRouterAccountPanel: View {
    @ObservedObject var vm: SettingsViewModel
    @State private var revealKey = false
    @State private var testStatus = ""
    @State private var testIsError = false
    @State private var testing = false

    var body: some View {
        HStack {
            Text("BYO key. You pay OpenRouter directly. Get a key at openrouter.ai/keys.")
                .font(.callout)
                .foregroundColor(.secondary)
            Spacer()
            if vm.hasOpenRouterAPIKey {
                Label("Saved", systemImage: "checkmark.seal.fill")
                    .font(.caption)
                    .foregroundStyle(.green)
            }
        }

        HStack(spacing: 8) {
            Group {
                if revealKey {
                    TextField("sk-or-… (paste to replace, leave empty to keep saved)", text: $vm.openRouterAPIKey)
                } else {
                    SecureField("sk-or-… (paste to replace, leave empty to keep saved)", text: $vm.openRouterAPIKey)
                }
            }
            .textFieldStyle(.roundedBorder)
            .font(.system(.body, design: .monospaced))

            Button {
                if !revealKey {
                    vm.loadOpenRouterAPIKeyIfNeeded()
                }
                revealKey.toggle()
            } label: {
                Image(systemName: revealKey ? "eye.slash" : "eye")
            }
            .help(revealKey ? "Hide key" : "Show saved key")
        }

        HStack(spacing: 10) {
            Button("Save") {
                if vm.saveOpenRouterAPIKey() {
                    testStatus = "Saved."
                    testIsError = false
                } else {
                    testStatus = "Save failed — couldn't write to secrets.json."
                    testIsError = true
                }
            }
            .disabled(vm.openRouterAPIKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

            Button(testing ? "Testing…" : "Test connection") {
                testing = true
                testStatus = ""
                vm.testOpenRouterConnection { ok, msg in
                    testing = false
                    testStatus = msg
                    testIsError = !ok
                }
            }
            .disabled(testing || (!vm.hasOpenRouterAPIKey && vm.openRouterAPIKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty))

            if !testStatus.isEmpty {
                Text(testStatus)
                    .font(.callout)
                    .foregroundColor(testIsError ? .red : .secondary)
                    .lineLimit(2)
            }
        }
    }
}

/// Wispr Flow's Supabase OAuth lives behind the same shape as OpenRouter's
/// BYO key and Claude Voice's CLI keychain entry — it's per-vendor auth,
/// not a universal app account, so it belongs in the Accounts tab.
private struct WisprFlowAccountPanel: View {
    let session: Session
    @State private var isSignedIn = false
    @State private var displayName = ""
    @State private var email = ""
    @State private var avatarURL: String? = nil

    var body: some View {
        Text("Sign in with your Wispr Flow account to use Flow's WebSocket transcription pipeline. Auth is shared with the official Wispr Flow desktop app via a Supabase session file.")
            .font(.callout)
            .foregroundColor(.secondary)

        Group {
            if isSignedIn {
                HStack(spacing: Theme.Spacing.medium) {
                    Group {
                        if let urlString = avatarURL, let url = URL(string: urlString) {
                            AsyncImage(url: url) { image in
                                image.resizable().scaledToFill()
                            } placeholder: {
                                Image(systemName: "person.crop.circle.fill")
                                    .foregroundStyle(.secondary)
                            }
                            .frame(width: 32, height: 32)
                            .clipShape(Circle())
                        } else {
                            Image(systemName: "person.crop.circle.fill")
                                .font(.title2)
                                .foregroundStyle(.secondary)
                        }
                    }
                    VStack(alignment: .leading, spacing: 2) {
                        if !displayName.isEmpty && displayName != email {
                            Text(displayName).font(.body.weight(.medium))
                        }
                        Text(email).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Sign Out") {
                        session.clear()
                        NotificationCenter.default.post(name: .sessionChanged, object: nil)
                    }
                    .controlSize(.small)
                }
            } else {
                HStack(spacing: Theme.Spacing.medium) {
                    Image(systemName: "person.crop.circle.badge.questionmark")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                    Text("Not signed in").foregroundStyle(.secondary)
                    Spacer()
                    Button("Sign In with Google") {
                        AuthService.signInWithBrowser()
                    }
                    .controlSize(.small)
                }
            }
        }
        .onAppear { refresh() }
        .onReceive(NotificationCenter.default.publisher(for: .sessionChanged)) { _ in refresh() }
    }

    private func refresh() {
        isSignedIn = session.isValid
        email = session.userEmail ?? ""
        avatarURL = session.avatarURL
        let first = session.userFirstName ?? ""
        let last = session.userLastName ?? ""
        let full = [first, last].filter { !$0.isEmpty }.joined(separator: " ")
        displayName = full.isEmpty ? email : full
    }
}

/// Inline check of the `Claude Code-credentials` Keychain entry. Deliberately
/// not part of `PermissionStatusPoller` because the first Keychain read after
/// a fresh launch triggers a macOS password dialog — we let the user fire it
/// explicitly with the "Check" button instead of at view-appear time.
private struct ClaudeVoiceAuthRow: View {
    @StateObject private var auth = ClaudeVoiceAuthCheck()
    @State private var cliInstalled: Bool = ClaudeCodeKeychain.isCLIInstalled

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
            Text("Sends audio live to Claude Code's STT WebSocket. Auth uses the OAuth token the `claude` CLI stores in your Keychain — Wispr Lightning never writes to it.")
                .font(.callout)
                .foregroundColor(.secondary)

            if !cliInstalled {
                HStack(spacing: 12) {
                    Image(systemName: "info.circle.fill")
                        .foregroundStyle(.blue)
                        .font(.title2)
                        .frame(width: 24)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Claude CLI not detected").font(.body.bold())
                        Text("Lightning's Claude Voice provider needs the `claude` CLI. Install it from claude.ai/download, then run `claude /login` to sign in.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Spacer()
                    Button("Open download page") {
                        if let url = URL(string: "https://claude.ai/download") {
                            NSWorkspace.shared.open(url)
                        }
                    }
                    .controlSize(.small)
                }
            }

            HStack(spacing: 12) {
                Image(systemName: iconName)
                    .foregroundStyle(iconColor)
                    .font(.title2)
                    .frame(width: 24)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Claude Code sign-in").font(.body.bold())
                    Text(rationale)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer()
                actionButtons
            }
        }
        .onAppear { cliInstalled = ClaudeCodeKeychain.isCLIInstalled }
    }

    @ViewBuilder
    private var actionButtons: some View {
        switch auth.state {
        case .signedIn:
            Text("Signed in").font(.caption).foregroundStyle(.green)
        case .checking:
            ProgressView().controlSize(.small)
        case .unchecked:
            Button("Check") { auth.check() }.controlSize(.small)
        case .expired, .notSignedIn:
            HStack(spacing: 6) {
                Button("Copy command") { copyLoginCommand() }.controlSize(.small)
                Button("Re-check") { auth.check() }.controlSize(.small)
            }
        }
    }

    private func copyLoginCommand() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString("claude /login", forType: .string)
    }

    private var iconName: String {
        switch auth.state {
        case .signedIn: return "checkmark.circle.fill"
        case .checking, .unchecked: return "questionmark.circle.fill"
        case .expired, .notSignedIn: return "exclamationmark.circle.fill"
        }
    }

    private var iconColor: Color {
        switch auth.state {
        case .signedIn: return .green
        case .checking, .unchecked: return .secondary
        case .expired, .notSignedIn: return .orange
        }
    }

    private var rationale: String {
        switch auth.state {
        case .unchecked:
            return "Reads the OAuth token the `claude` CLI stored in your Keychain. macOS may ask for your login password the first time."
        case .checking:
            return "Reading Keychain…"
        case .signedIn:
            return "Token found and valid."
        case .expired:
            return "Token expired — run `claude /login` in a terminal."
        case .notSignedIn:
            return "No token found — run `claude /login` in a terminal."
        }
    }
}

private final class ClaudeVoiceAuthCheck: ObservableObject {
    enum State: Equatable { case unchecked, checking, signedIn, expired, notSignedIn }
    @Published private(set) var state: State = .unchecked

    func check() {
        guard state != .checking else { return }
        state = .checking
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let next: State
            // Drop both in-process and on-disk caches so a fresh upstream read
            // is forced — necessary after the user runs `claude /login` again.
            ClaudeCodeKeychain.clearAllCaches()
            do {
                let token = try ClaudeCodeKeychain.read(forceRefresh: true)
                next = token.isExpired ? .expired : .signedIn
            } catch {
                next = .notSignedIn
            }
            DispatchQueue.main.async {
                self?.state = next
                // The Keychain password dialog steals focus away from us; pull
                // it back so the Settings window doesn't feel like it vanished.
                NSApp.activate(ignoringOtherApps: true)
            }
        }
    }
}

// MARK: - Polish Detail

private struct PolishDetail: View {
    @ObservedObject var vm: SettingsViewModel

    var body: some View {
        Text("Polish")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
                SettingsToggleRow("Enable Polish",
                    description: "Refine selected text with AI",
                    isOn: $vm.polishEnabled)
                    .onChange(of: vm.polishEnabled) { _ in vm.savePolishSettings() }

                if vm.polishEnabled {
                    VStack(alignment: .leading, spacing: Theme.Spacing.small) {
                        Text("Polish hotkey:")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)

                        ForEach(Array(vm.polishHotkeyLabels.enumerated()), id: \.offset) { index, label in
                            HStack(spacing: Theme.Spacing.medium) {
                                KeyCapView(label: label)

                                if vm.polishHotkeyLabels.count > 1 {
                                    Button {
                                        vm.removePolishHotkey(at: index)
                                    } label: {
                                        Image(systemName: "minus.circle")
                                            .foregroundStyle(.red)
                                    }
                                    .buttonStyle(.borderless)
                                    .help("Remove this polish hotkey")
                                }
                            }
                        }

                        Button(vm.isCapturingPolishShortcut ? "Press a key…" : "Add Polish Hotkey") {
                            vm.startCapturingPolishHotkey()
                        }
                        .controlSize(.small)
                    }

                    Divider()

                    Text("Polish instructions:")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)

                    ForEach(Array(vm.polishInstructions.keys.sorted()), id: \.self) { key in
                        Toggle(isOn: Binding(
                            get: { vm.polishInstructions[key] ?? false },
                            set: { newValue in
                                vm.polishInstructions[key] = newValue
                                vm.savePolishSettings()
                            }
                        )) {
                            Text(key)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .toggleStyle(.switch)
                        .controlSize(.small)
                    }

                    Divider()

                    SettingsToggleRow("Auto-polish after dictation",
                        description: "Automatically polish text after each dictation",
                        isOn: $vm.autoPolish)
                        .onChange(of: vm.autoPolish) { _ in vm.savePolishSettings() }
                }
            }
            .padding(Theme.Spacing.medium)
        }
    }
}

// MARK: - Personalization Detail

private struct PersonalizationDetail: View {
    @ObservedObject var vm: SettingsViewModel

    static let styleOptions = ["default", "formal", "casual", "friendly", "professional"]
    static let contexts: [(key: String, label: String)] = [
        ("work", "Work"),
        ("email", "Email"),
        ("personal", "Personal"),
        ("other", "Other"),
    ]

    var body: some View {
        Text("Personalization")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
                SettingsToggleRow("Style detection",
                    description: "Automatically adjust tone based on context",
                    isOn: $vm.styleDetectionEnabled)
                    .onChange(of: vm.styleDetectionEnabled) { _ in vm.savePersonalizationSettings() }

                if vm.styleDetectionEnabled {
                    ForEach(Self.contexts, id: \.key) { ctx in
                        Picker(ctx.label, selection: Binding(
                            get: { vm.personalizationStyles[ctx.key] ?? "default" },
                            set: { newValue in
                                vm.personalizationStyles[ctx.key] = newValue
                                vm.savePersonalizationSettings()
                            }
                        )) {
                            ForEach(Self.styleOptions, id: \.self) { option in
                                Text(option.capitalized).tag(option)
                            }
                        }
                        .pickerStyle(.menu)
                    }
                }
            }
            .padding(Theme.Spacing.medium)
        }
    }
}

// MARK: - Privacy Detail

private struct PrivacyDetail: View {
    @ObservedObject var vm: SettingsViewModel

    var body: some View {
        Text("Privacy")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
                SettingsToggleRow("Screen context (OCR)",
                    description: "Capture screen text for context-aware formatting",
                    isOn: $vm.useScreenContext)
                    .onChange(of: vm.useScreenContext) { _ in vm.savePrivacySettings() }

                SettingsToggleRow("Accessibility context",
                    description: "Use accessibility APIs for better transcription context",
                    isOn: $vm.useAccessibilityContext)
                    .onChange(of: vm.useAccessibilityContext) { _ in vm.savePrivacySettings() }

                SettingsToggleRow("Share anonymous usage data",
                    description: "Help improve Wispr by sharing anonymous statistics",
                    isOn: $vm.shareUsageData)
                    .onChange(of: vm.shareUsageData) { _ in vm.savePrivacySettings() }
            }
            .padding(Theme.Spacing.medium)
        }

        Text("Where your audio goes")
            .font(.headline)
            .padding(.top, Theme.Spacing.medium)
        VStack(alignment: .leading, spacing: 8) {
            DataFlowRow(
                vendor: "Wispr Flow",
                detail: "Audio uploads over WebSocket to api.wisprflow.ai for transcription and AI cleanup. Subject to Wispr Flow's privacy policy. Your account is used to bill / track usage."
            )
            DataFlowRow(
                vendor: "OpenRouter",
                detail: "Audio is sent inline as base64 WAV in an HTTPS request to openrouter.ai, which routes it to the model you picked (Google, Anthropic, etc.). Billed to your OpenRouter account; subject to OpenRouter's and the underlying model provider's privacy policies."
            )
            DataFlowRow(
                vendor: "Claude Voice",
                detail: "Audio streams live over WebSocket to api.anthropic.com using the OAuth token the `claude` CLI manages. Subject to Anthropic's privacy policy."
            )
        }

        Text("Where credentials live")
            .font(.headline)
            .padding(.top, Theme.Spacing.medium)
        VStack(alignment: .leading, spacing: 6) {
            Text("• **Wispr Flow** session token: `~/Library/Application Support/WisprLightning/session.json` (file owner only).")
            Text("• **OpenRouter** API key: `~/Library/Application Support/WisprLightning/secrets/secrets.json`, dir mode 0700, file mode 0600.")
            Text("• **Claude Voice** OAuth token: read from the `claude` CLI's `Claude Code-credentials` Keychain item; mirrored in the same secrets.json above for silent reads.")
        }
        .font(.callout)
        .foregroundStyle(.secondary)
    }
}

private struct DataFlowRow: View {
    let vendor: String
    let detail: String

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "antenna.radiowaves.left.and.right")
                .foregroundStyle(.secondary)
                .frame(width: 20)
            VStack(alignment: .leading, spacing: 2) {
                Text(vendor).font(.body.weight(.medium))
                Text(detail)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(10)
        .background(Color(NSColor.controlBackgroundColor))
        .cornerRadius(8)
    }
}

// MARK: - System Detail

private struct SystemDetail: View {
    @ObservedObject var vm: SettingsViewModel

    var body: some View {
        Text("System")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
                SettingsToggleRow("Launch at login", isOn: $vm.launchAtLogin)
                    .onChange(of: vm.launchAtLogin) { _ in vm.saveSystemSettings(); vm.updateLaunchAgent() }

                SettingsToggleRow("Show in Dock", isOn: $vm.showInDock)
                    .onChange(of: vm.showInDock) { _ in
                        vm.saveSystemSettings()
                        NSApp.setActivationPolicy(vm.showInDock ? .regular : .accessory)
                    }

                SettingsToggleRow("Sound effects", isOn: $vm.enableSounds)
                    .onChange(of: vm.enableSounds) { _ in vm.saveSystemSettings() }

                SettingsToggleRow("Mute music while dictating", isOn: $vm.muteMusic)
                    .onChange(of: vm.muteMusic) { _ in vm.saveSystemSettings() }

                Divider()

                SettingsToggleRow("Verbose logging",
                    description: "Log full server requests and responses to ~/Library/Logs/WisprLightning.log",
                    isOn: $vm.verboseLogging)
                    .onChange(of: vm.verboseLogging) { _ in vm.saveSystemSettings() }

                Divider()

                HStack {
                    Picker("Sound pack", selection: $vm.selectedSoundPack) {
                        Text("Default").tag(String?.none)
                        ForEach(vm.availableSoundPacks.filter { $0 != "default" }, id: \.self) { pack in
                            Text(pack.capitalized).tag(Optional(pack))
                        }
                    }
                    .onChange(of: vm.selectedSoundPack) { _ in vm.saveSystemSettings() }

                    Button("Preview") { vm.previewSoundPack() }
                        .controlSize(.small)
                }
            }
            .padding(Theme.Spacing.medium)
        }

        Divider()

        Text("Wispr Lightning v1.0.0")
            .font(.subheadline)
            .foregroundStyle(.tertiary)
    }
}

// MARK: - Key Cap View

struct KeyCapView: View {
    let label: String

    var body: some View {
        Text(label)
            .font(.system(.body, design: .monospaced).weight(.medium))
            .frame(minWidth: 40)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(Color(nsColor: .controlBackgroundColor))
            .cornerRadius(6)
            .overlay(
                RoundedRectangle(cornerRadius: 6)
                    .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
            )
    }
}

// MARK: - View Model

class SettingsViewModel: ObservableObject {
    let settings: AppSettings

    @Published var isCapturingShortcut = false
    /// Convenience for UI live tests — the live set of bound dictation keycodes.
    var hotkeyKeyCodesSet: Set<UInt16> { Set(settings.hotkeyKeyCodes) }
    @Published var hotkeyLabels: [String] = []
    @Published var hotkeyTapToToggle: Bool
    @Published var hotkeyPressBehavior: String
    @Published var selectedMicUID: String?
    @Published var keepMicrophoneActive: Bool
    @Published var selectedLanguages: Set<String>
    @Published var launchAtLogin: Bool
    @Published var showInDock: Bool
    @Published var enableSounds: Bool
    @Published var muteMusic: Bool
    @Published var aiFormatting: Bool
    @Published var autoCleanupLevel: String
    @Published var commandModeEnabled: Bool
    @Published var hyperlinkOn: Bool
    @Published var autoLearnWords: Bool
    @Published var styleDetectionEnabled: Bool
    @Published var personalizationStyles: [String: String]
    @Published var useScreenContext: Bool
    @Published var useAccessibilityContext: Bool
    @Published var shareUsageData: Bool
    @Published var micDevices: [(uid: String, name: String)] = []

    // Polish
    @Published var polishEnabled: Bool
    @Published var polishInstructions: [String: Bool]
    @Published var autoPolish: Bool
    @Published var polishHotkeyLabels: [String]

    // Polish hotkey capture
    @Published var isCapturingPolishShortcut = false
    private var polishShortcutMonitor: Any?

    // Email Signatures
    @Published var emailAutoSignature: Bool
    @Published var emailSignatureOption: String

    // Creator Mode
    @Published var creatorMode: Bool

    // Natural Mode
    @Published var naturalModeEnabled: Bool
    @Published var naturalModeSpeed: String

    // Sound Packs
    @Published var selectedSoundPack: String?
    @Published var availableSoundPacks: [String] = []

    // Debug
    @Published var verboseLogging: Bool

    // Provider (transcription vendor)
    @Published var activeVendor: String
    @Published var openRouterModel: String
    @Published var openRouterAPIKey: String
    @Published var openRouterModelList: [OpenRouterAudioModel] = []
    @Published var openRouterModelListState: OpenRouterModelListState = .idle
    @Published var fallbackChain: [FallbackStep] = []

    enum OpenRouterModelListState: Equatable {
        case idle
        case loading
        case loaded
        case failed(String)
    }

    private var shortcutMonitor: Any?

    deinit {
        if let monitor = shortcutMonitor {
            NSEvent.removeMonitor(monitor)
        }
        if let monitor = polishShortcutMonitor {
            NSEvent.removeMonitor(monitor)
        }
    }

    struct Language {
        let code: String
        let name: String
        let flag: String
    }

    static let cleanupLevels: [(value: String, label: String)] = [
        ("none", "None"),
        ("light", "Light"),
        ("heavy", "Heavy"),
    ]

    static let autoDetectCode = "auto"

    static let languages: [Language] = [
        .init(code: "en", name: "English", flag: "🇺🇸"),
        .init(code: "engb", name: "English — British", flag: "🇬🇧"),
        .init(code: "zh", name: "Chinese — Traditional (繁體中文)", flag: "🇹🇼"),
        .init(code: "zhcn", name: "Chinese — Simplified (简体中文)", flag: "🇨🇳"),
        .init(code: "de", name: "German (Deutsch)", flag: "🇩🇪"),
        .init(code: "dech", name: "German — Swiss (Deutsch)", flag: "🇨🇭"),
        .init(code: "es", name: "Spanish (Español)", flag: "🇪🇸"),
        .init(code: "ru", name: "Russian (Русский)", flag: "🇷🇺"),
        .init(code: "ko", name: "Korean (한국어)", flag: "🇰🇷"),
        .init(code: "fr", name: "French (Français)", flag: "🇫🇷"),
        .init(code: "ja", name: "Japanese (日本語)", flag: "🇯🇵"),
        .init(code: "pt", name: "Portuguese (Português)", flag: "🇧🇷"),
        .init(code: "tr", name: "Turkish (Türkçe)", flag: "🇹🇷"),
        .init(code: "pl", name: "Polish (Polski)", flag: "🇵🇱"),
        .init(code: "ca", name: "Catalan (Català)", flag: "🇪🇸"),
        .init(code: "nl", name: "Dutch (Nederlands)", flag: "🇳🇱"),
        .init(code: "ar", name: "Arabic (العربية)", flag: "🇸🇦"),
        .init(code: "sv", name: "Swedish (Svenska)", flag: "🇸🇪"),
        .init(code: "it", name: "Italian (Italiano)", flag: "🇮🇹"),
        .init(code: "id", name: "Indonesian (Bahasa)", flag: "🇮🇩"),
        .init(code: "hi", name: "Hindi (हिन्दी)", flag: "🇮🇳"),
        .init(code: "hien", name: "Hinglish", flag: "🇮🇳"),
        .init(code: "fi", name: "Finnish (Suomi)", flag: "🇫🇮"),
        .init(code: "vi", name: "Vietnamese (Tiếng Việt)", flag: "🇻🇳"),
        .init(code: "he", name: "Hebrew (עברית)", flag: "🇮🇱"),
        .init(code: "uk", name: "Ukrainian (Українська)", flag: "🇺🇦"),
        .init(code: "el", name: "Greek (Ελληνικά)", flag: "🇬🇷"),
        .init(code: "ms", name: "Malay (Bahasa Melayu)", flag: "🇲🇾"),
        .init(code: "cs", name: "Czech (Čeština)", flag: "🇨🇿"),
        .init(code: "ro", name: "Romanian (Română)", flag: "🇷🇴"),
        .init(code: "da", name: "Danish (Dansk)", flag: "🇩🇰"),
        .init(code: "hu", name: "Hungarian (Magyar)", flag: "🇭🇺"),
        .init(code: "ta", name: "Tamil (தமிழ்)", flag: "🇮🇳"),
        .init(code: "no", name: "Norwegian (Norsk)", flag: "🇳🇴"),
        .init(code: "th", name: "Thai (ไทย)", flag: "🇹🇭"),
        .init(code: "ur", name: "Urdu (اردو)", flag: "🇵🇰"),
        .init(code: "hr", name: "Croatian (Hrvatski)", flag: "🇭🇷"),
        .init(code: "bg", name: "Bulgarian (Български)", flag: "🇧🇬"),
        .init(code: "lt", name: "Lithuanian (Lietuvių)", flag: "🇱🇹"),
        .init(code: "la", name: "Latin (Latina)", flag: "🌍"),
        .init(code: "mi", name: "Maori", flag: "🇳🇿"),
        .init(code: "ml", name: "Malayalam (മലയാളം)", flag: "🇮🇳"),
        .init(code: "cy", name: "Welsh (Cymraeg)", flag: "🏴󠁧󠁢󠁷󠁬󠁳󠁿"),
        .init(code: "sk", name: "Slovak (Slovenčina)", flag: "🇸🇰"),
        .init(code: "te", name: "Telugu (తెలుగు)", flag: "🇮🇳"),
        .init(code: "fa", name: "Persian (فارسی)", flag: "🇮🇷"),
        .init(code: "lv", name: "Latvian (Latviešu)", flag: "🇱🇻"),
        .init(code: "bn", name: "Bengali (বাংলা)", flag: "🇧🇩"),
        .init(code: "sr", name: "Serbian (Српски)", flag: "🇷🇸"),
        .init(code: "az", name: "Azerbaijani (Azərbaycan)", flag: "🇦🇿"),
        .init(code: "sl", name: "Slovenian (Slovenščina)", flag: "🇸🇮"),
        .init(code: "kn", name: "Kannada (ಕನ್ನಡ)", flag: "🇮🇳"),
        .init(code: "et", name: "Estonian (Eesti)", flag: "🇪🇪"),
        .init(code: "mk", name: "Macedonian (Македонски)", flag: "🇲🇰"),
        .init(code: "br", name: "Breton (Brezhoneg)", flag: "🇫🇷"),
        .init(code: "eu", name: "Basque (Euskara)", flag: "🇪🇸"),
        .init(code: "is", name: "Icelandic (Íslenska)", flag: "🇮🇸"),
        .init(code: "hy", name: "Armenian (Հայերեն)", flag: "🇦🇲"),
        .init(code: "ne", name: "Nepali (नेपाली)", flag: "🇳🇵"),
        .init(code: "mn", name: "Mongolian (Монгол)", flag: "🇲🇳"),
        .init(code: "bs", name: "Bosnian (Bosanski)", flag: "🇧🇦"),
        .init(code: "kk", name: "Kazakh (Қазақша)", flag: "🇰🇿"),
        .init(code: "sq", name: "Albanian (Shqip)", flag: "🇦🇱"),
        .init(code: "sw", name: "Swahili (Kiswahili)", flag: "🇹🇿"),
        .init(code: "gl", name: "Galician (Galego)", flag: "🇪🇸"),
        .init(code: "mr", name: "Marathi (मराठी)", flag: "🇮🇳"),
        .init(code: "pa", name: "Punjabi (ਪੰਜਾਬੀ)", flag: "🇮🇳"),
        .init(code: "si", name: "Sinhala (සිංහල)", flag: "🇱🇰"),
        .init(code: "km", name: "Khmer (ខ្មែរ)", flag: "🇰🇭"),
        .init(code: "sn", name: "Shona (chiShona)", flag: "🇿🇼"),
        .init(code: "yo", name: "Yoruba", flag: "🇳🇬"),
        .init(code: "so", name: "Somali (Soomaali)", flag: "🇸🇴"),
        .init(code: "af", name: "Afrikaans", flag: "🇿🇦"),
        .init(code: "oc", name: "Occitan", flag: "🌍"),
        .init(code: "ka", name: "Georgian (ქართული)", flag: "🇬🇪"),
        .init(code: "be", name: "Belarusian (Беларуская)", flag: "🇧🇾"),
        .init(code: "tg", name: "Tajik (Тоҷикӣ)", flag: "🇹🇯"),
        .init(code: "sd", name: "Sindhi (سنڌي)", flag: "🇵🇰"),
        .init(code: "gu", name: "Gujarati (ગુજરાતી)", flag: "🇮🇳"),
        .init(code: "am", name: "Amharic (አማርኛ)", flag: "🇪🇹"),
        .init(code: "yi", name: "Yiddish (ייִדיש)", flag: "🌍"),
        .init(code: "lo", name: "Lao (ລາວ)", flag: "🇱🇦"),
        .init(code: "uz", name: "Uzbek (Oʻzbek)", flag: "🇺🇿"),
        .init(code: "fo", name: "Faroese (Føroyskt)", flag: "🇫🇴"),
        .init(code: "ht", name: "Haitian Creole (Kreyòl Ayisyen)", flag: "🇭🇹"),
        .init(code: "ps", name: "Pashto (پښتو)", flag: "🇦🇫"),
        .init(code: "tk", name: "Turkmen", flag: "🇹🇲"),
        .init(code: "nn", name: "Nynorsk", flag: "🇳🇴"),
        .init(code: "mt", name: "Maltese (Malti)", flag: "🇲🇹"),
        .init(code: "sa", name: "Sanskrit (संस्कृतम्)", flag: "🇮🇳"),
        .init(code: "lb", name: "Luxembourgish (Lëtzebuergesch)", flag: "🇱🇺"),
        .init(code: "my", name: "Myanmar (မြန်မာ)", flag: "🇲🇲"),
        .init(code: "bo", name: "Tibetan (བོད་སྐད)", flag: "🌍"),
        .init(code: "tl", name: "Tagalog", flag: "🇵🇭"),
        .init(code: "mg", name: "Malagasy", flag: "🇲🇬"),
        .init(code: "as", name: "Assamese (অসমীয়া)", flag: "🇮🇳"),
        .init(code: "tt", name: "Tatar (Татар)", flag: "🇷🇺"),
        .init(code: "haw", name: "Hawaiian (ʻŌlelo Hawaiʻi)", flag: "🇺🇸"),
        .init(code: "ln", name: "Lingala", flag: "🇨🇩"),
        .init(code: "ha", name: "Hausa", flag: "🇳🇬"),
        .init(code: "ba", name: "Bashkir (Башҡортса)", flag: "🇷🇺"),
        .init(code: "jv", name: "Javanese (Basa Jawa)", flag: "🇮🇩"),
        .init(code: "su", name: "Sundanese (Basa Sunda)", flag: "🇮🇩"),
        .init(code: "yue", name: "Cantonese (粵語)", flag: "🇭🇰"),
    ]

    init(settings: AppSettings) {
        self.settings = settings
        self.selectedMicUID = settings.micDeviceUID
        self.keepMicrophoneActive = settings.keepMicrophoneActive
        self.selectedLanguages = Set(settings.languages)
        self.launchAtLogin = settings.launchAtLogin
        self.showInDock = settings.showInDock
        self.enableSounds = settings.enableSounds
        self.muteMusic = settings.muteMusic
        self.aiFormatting = settings.aiFormatting
        self.autoCleanupLevel = settings.autoCleanupLevel
        self.commandModeEnabled = settings.commandModeEnabled
        self.hyperlinkOn = settings.hyperlinkOn
        self.autoLearnWords = settings.autoLearnWords
        self.styleDetectionEnabled = settings.styleDetectionEnabled
        self.personalizationStyles = settings.personalizationStyles
        self.useScreenContext = settings.useScreenContext
        self.useAccessibilityContext = settings.useAccessibilityContext
        self.shareUsageData = settings.shareUsageData
        self.hotkeyLabels = settings.hotkeyLabels.isEmpty ? ["Left Control"] : settings.hotkeyLabels
        self.hotkeyTapToToggle = settings.hotkeyTapToToggle
        self.hotkeyPressBehavior = settings.hotkeyPressBehavior.isEmpty
            ? (settings.hotkeyTapToToggle ? "toggle" : "legacy")
            : settings.hotkeyPressBehavior

        // Polish
        self.polishEnabled = settings.polishEnabled
        self.polishInstructions = settings.polishInstructions
        self.autoPolish = settings.autoPolish
        self.polishHotkeyLabels = settings.polishHotkeyLabels

        // Email Signatures
        self.emailAutoSignature = settings.emailAutoSignature
        self.emailSignatureOption = settings.emailSignatureOption

        // Creator Mode
        self.creatorMode = settings.creatorMode

        // Natural Mode
        self.naturalModeEnabled = settings.naturalModeEnabled
        self.naturalModeSpeed = settings.naturalModeSpeed

        // Sound Packs
        self.selectedSoundPack = settings.selectedSoundPack

        // Debug
        self.verboseLogging = settings.verboseLogging

        // Provider
        self.activeVendor = settings.activeVendor
        self.openRouterModel = settings.openRouterModel
        // Defer the Keychain read until the user actually opens the OpenRouter
        // panel — opening Settings shouldn't trigger a password prompt if the
        // user only came to change a hotkey or pick a vendor.
        self.openRouterAPIKey = ""
        self.fallbackChain = settings.fallbackChain

        refreshMicDevices()
        availableSoundPacks = SoundManager.availablePacks()
    }

    // MARK: - Provider

    func saveActiveVendor() {
        settings.activeVendor = activeVendor
        settings.save()
    }

    func saveProviderSettings() {
        // Persists activeVendor + openRouterModel only. The API key is saved
        // separately via saveOpenRouterAPIKey() so model-picker changes can't
        // accidentally overwrite a stored key with whatever's in the (often
        // empty) input field.
        settings.activeVendor = activeVendor
        settings.openRouterModel = openRouterModel.trimmingCharacters(in: .whitespacesAndNewlines)
        settings.save()
        NotificationCenter.default.post(name: .settingsChanged, object: settings)
    }

    /// Persists the OpenRouter API key. Triggered by the explicit Save button
    /// in the Accounts panel only. Empty input is treated as "keep the
    /// existing value" rather than "delete" so the user doesn't lose their
    /// stored key by hitting Save with a blank field. Returns the write
    /// result so the UI can surface "Saved." vs "Save failed".
    @discardableResult
    func saveOpenRouterAPIKey() -> Bool {
        let trimmed = openRouterAPIKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return false }
        return SecretsStore.write(.openRouterAPIKey, trimmed)
    }

    /// Load the OpenRouter API key from disk on demand. SecretsStore is
    /// file-backed, so this never triggers a Keychain prompt.
    private var openRouterKeyLoaded = false
    func loadOpenRouterAPIKeyIfNeeded() {
        guard !openRouterKeyLoaded else { return }
        openRouterKeyLoaded = true
        openRouterAPIKey = SecretsStore.read(.openRouterAPIKey) ?? ""
    }

    /// True iff a saved key exists, without revealing its value. Used to
    /// show "saved ✓" in the Accounts panel without triggering any access.
    var hasOpenRouterAPIKey: Bool {
        return SecretsStore.has(.openRouterAPIKey)
    }

    // MARK: - Fallback chain

    func addFallbackStep() {
        // Default new step to whatever vendor the user isn't already on.
        let existingVendors = Set(fallbackChain.map { $0.vendor }).union([activeVendor])
        let candidate = DictationVendor.allCases.first { !existingVendors.contains($0.rawValue) }
            ?? .openRouter
        fallbackChain.append(FallbackStep(vendor: candidate.rawValue))
        saveFallbackChain()
    }

    func removeFallbackStep(at index: Int) {
        guard fallbackChain.indices.contains(index) else { return }
        fallbackChain.remove(at: index)
        saveFallbackChain()
    }

    func moveFallbackStep(from src: Int, to dst: Int) {
        guard fallbackChain.indices.contains(src),
              dst >= 0, dst <= fallbackChain.count, src != dst else { return }
        let step = fallbackChain.remove(at: src)
        let insertAt = dst > src ? dst - 1 : dst
        fallbackChain.insert(step, at: insertAt)
        saveFallbackChain()
    }

    /// Promote a chain step into the primary slot, demoting the current
    /// primary into that step's old position. Lets the user reorder the
    /// whole list — including row #1 — without a separate "make primary"
    /// affordance.
    func promoteToPrimary(at chainIndex: Int) {
        guard fallbackChain.indices.contains(chainIndex) else { return }
        let promoted = fallbackChain[chainIndex]

        // Capture the old primary as a chain step. Carry its OpenRouter model
        // override only if it actually was OpenRouter — otherwise the field
        // is meaningless and would leak into a different vendor's slot.
        let demoted = FallbackStep(
            vendor: activeVendor,
            openRouterModel: activeVendor == DictationVendor.openRouter.rawValue ? openRouterModel : nil
        )
        fallbackChain[chainIndex] = demoted

        // Promote the chain step. If it carried its own OpenRouter model,
        // adopt that as the primary model so the dropdown reflects it.
        activeVendor = promoted.vendor
        if promoted.vendor == DictationVendor.openRouter.rawValue,
           let m = promoted.openRouterModel, !m.isEmpty {
            openRouterModel = m
        }

        settings.activeVendor = activeVendor
        settings.openRouterModel = openRouterModel
        settings.fallbackChain = fallbackChain
        settings.save()
    }

    /// Demote the primary into the chain — swaps with the existing chain[0]
    /// (or appends if the chain is empty). Mirror of promoteToPrimary so
    /// users can move the primary "down".
    func demotePrimary() {
        let demoted = FallbackStep(
            vendor: activeVendor,
            openRouterModel: activeVendor == DictationVendor.openRouter.rawValue ? openRouterModel : nil
        )
        if fallbackChain.isEmpty {
            // Nothing to swap with — just append the old primary and pick a
            // sane new primary (first vendor that isn't the old one).
            let next = DictationVendor.allCases.first { $0.rawValue != activeVendor } ?? .openRouter
            activeVendor = next.rawValue
            fallbackChain.append(demoted)
        } else {
            let promoted = fallbackChain[0]
            fallbackChain[0] = demoted
            activeVendor = promoted.vendor
            if promoted.vendor == DictationVendor.openRouter.rawValue,
               let m = promoted.openRouterModel, !m.isEmpty {
                openRouterModel = m
            }
        }
        settings.activeVendor = activeVendor
        settings.openRouterModel = openRouterModel
        settings.fallbackChain = fallbackChain
        settings.save()
    }

    func updateFallbackStepVendor(at index: Int, vendor: String) {
        guard fallbackChain.indices.contains(index) else { return }
        fallbackChain[index].vendor = vendor
        if vendor != DictationVendor.openRouter.rawValue {
            fallbackChain[index].openRouterModel = nil
        }
        saveFallbackChain()
    }

    func updateFallbackStepModel(at index: Int, model: String?) {
        guard fallbackChain.indices.contains(index) else { return }
        let cleaned = model?.trimmingCharacters(in: .whitespaces)
        fallbackChain[index].openRouterModel = (cleaned?.isEmpty == false) ? cleaned : nil
        saveFallbackChain()
    }

    private func saveFallbackChain() {
        settings.fallbackChain = fallbackChain
        settings.save()
    }

    /// Async fetch of all audio-input models from OpenRouter. Cheap-first.
    /// No-op if a load is already in progress or completed in this session.
    func loadOpenRouterModels(force: Bool = false) {
        if !force {
            if case .loading = openRouterModelListState { return }
            if case .loaded = openRouterModelListState { return }
        }
        openRouterModelListState = .loading
        OpenRouterModels.fetchAudioModels { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let models):
                self.openRouterModelList = models
                self.openRouterModelListState = .loaded
            case .failure(let err):
                self.openRouterModelListState = .failed(err.localizedDescription)
            }
        }
    }

    /// Pings OpenRouter to verify the entered key. Calls completion on the main
    /// thread with (success, message).
    func testOpenRouterConnection(_ completion: @escaping (Bool, String) -> Void) {
        // Prefer the freshly-typed value in the field; fall back to the
        // saved key so Test works without making the user re-paste.
        let typed = openRouterAPIKey.trimmingCharacters(in: .whitespacesAndNewlines)
        let key = !typed.isEmpty ? typed : (SecretsStore.read(.openRouterAPIKey) ?? "")
        guard !key.isEmpty else {
            completion(false, "No API key saved or entered")
            return
        }
        var request = URLRequest(url: URL(string: "https://openrouter.ai/api/v1/auth/key")!)
        request.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = 15
        URLSession.shared.dataTask(with: request) { data, response, error in
            DispatchQueue.main.async {
                if let error = error {
                    completion(false, error.localizedDescription)
                    return
                }
                if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                    completion(false, "HTTP \(http.statusCode)")
                    return
                }
                guard let data = data,
                      let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                      let inner = json["data"] as? [String: Any] else {
                    completion(false, "Malformed response")
                    return
                }
                let label = (inner["label"] as? String) ?? "OK"
                let limit = inner["limit"] as? Double
                let usage = inner["usage"] as? Double ?? 0
                var msg = "Connected — key label: \(label)"
                if let limit = limit {
                    msg += "; usage $\(String(format: "%.2f", usage)) / $\(String(format: "%.2f", limit))"
                }
                completion(true, msg)
            }
        }.resume()
    }

    func refreshMicDevices() {
        micDevices = AudioRecorder.listInputDevices()
    }

    func saveHotkeyPressBehavior() {
        settings.hotkeyPressBehavior = hotkeyPressBehavior
        // Keep the legacy bool in sync so old code paths still work if they
        // happen to read it.
        settings.hotkeyTapToToggle = (hotkeyPressBehavior == "toggle")
        settings.save()
    }

    func saveMicSelection() {
        if let uid = selectedMicUID {
            settings.micDeviceUID = uid
            settings.micDeviceName = micDevices.first(where: { $0.uid == uid })?.name
        } else {
            settings.micDeviceUID = nil
            settings.micDeviceName = nil
        }
        settings.keepMicrophoneActive = keepMicrophoneActive
        settings.save()
    }

    var isAutoDetect: Bool {
        selectedLanguages.contains(Self.autoDetectCode)
    }

    func toggleLanguage(_ code: String) {
        if code == Self.autoDetectCode {
            // Auto-detect is exclusive — clears all others
            if isAutoDetect {
                selectedLanguages = ["en"] // fallback to English
            } else {
                selectedLanguages = [Self.autoDetectCode]
            }
        } else {
            // Selecting a specific language disables auto-detect
            selectedLanguages.remove(Self.autoDetectCode)
            if selectedLanguages.contains(code) {
                selectedLanguages.remove(code)
                if selectedLanguages.isEmpty {
                    selectedLanguages = ["en"] // always keep at least one
                }
            } else {
                selectedLanguages.insert(code)
            }
        }
        saveLanguages()
    }

    func saveLanguages() {
        settings.languages = Array(selectedLanguages)
        settings.save()
    }

    func saveDictationSettings() {
        settings.aiFormatting = aiFormatting
        settings.autoCleanupLevel = autoCleanupLevel
        settings.commandModeEnabled = commandModeEnabled
        settings.hyperlinkOn = hyperlinkOn
        settings.autoLearnWords = autoLearnWords
        settings.emailAutoSignature = emailAutoSignature
        settings.emailSignatureOption = emailSignatureOption
        settings.creatorMode = creatorMode
        settings.naturalModeEnabled = naturalModeEnabled
        settings.naturalModeSpeed = naturalModeSpeed
        settings.save()
    }

    func savePolishSettings() {
        settings.polishEnabled = polishEnabled
        settings.polishInstructions = polishInstructions
        settings.autoPolish = autoPolish
        settings.save()
    }

    func savePersonalizationSettings() {
        settings.styleDetectionEnabled = styleDetectionEnabled
        settings.personalizationStyles = personalizationStyles
        settings.save()
    }

    func savePrivacySettings() {
        settings.useScreenContext = useScreenContext
        settings.useAccessibilityContext = useAccessibilityContext
        settings.shareUsageData = shareUsageData
        settings.save()
    }

    func previewSoundPack() {
        saveSystemSettings()
        NotificationCenter.default.post(name: .settingsChanged, object: settings)
        // After SoundManager reloads the new pack, trigger a preview
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            NotificationCenter.default.post(name: .previewSoundPack, object: nil)
        }
    }

    func saveSystemSettings() {
        settings.launchAtLogin = launchAtLogin
        settings.showInDock = showInDock
        settings.enableSounds = enableSounds
        settings.muteMusic = muteMusic
        settings.selectedSoundPack = selectedSoundPack
        settings.verboseLogging = verboseLogging
        settings.save()
    }

    func removeHotkey(at index: Int) {
        guard hotkeyLabels.count > 1 else { return }
        var codes = settings.hotkeyKeyCodes
        var labels = settings.hotkeyLabels
        codes.remove(at: index)
        labels.remove(at: index)
        settings.hotkeyKeyCodes = codes
        settings.hotkeyLabels = labels
        settings.save()
        hotkeyLabels = labels
    }

    func startCapturing() {
        if isCapturingShortcut {
            stopCapturing()
            return
        }
        isCapturingShortcut = true

        shortcutMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { [weak self] event in
            guard let self = self else { return event }
            let keycode = event.keyCode

            // For flagsChanged, only capture on press, not release
            if event.type == .flagsChanged {
                guard HotkeyListener.isModifierDown(keycode: keycode, flags: event.modifierFlags) else { return nil }
            }

            let label: String
            if let knownLabel = HotkeyListener.keycodeLabels[keycode] {
                label = knownLabel
            } else {
                label = (event.charactersIgnoringModifiers ?? "?").uppercased()
            }

            // Don't add if already in the list
            guard !self.settings.hotkeyKeyCodes.contains(keycode) else {
                self.stopCapturing()
                return nil
            }

            var codes = self.settings.hotkeyKeyCodes
            var labels = self.settings.hotkeyLabels
            codes.append(keycode)
            labels.append(label)
            self.settings.hotkeyKeyCodes = codes
            self.settings.hotkeyLabels = labels
            self.settings.save()
            self.hotkeyLabels = labels
            self.stopCapturing()
            return nil
        }
    }

    private func stopCapturing() {
        isCapturingShortcut = false
        if let monitor = shortcutMonitor {
            NSEvent.removeMonitor(monitor)
            shortcutMonitor = nil
        }
    }

    // MARK: - Polish Hotkey Capture

    func removePolishHotkey(at index: Int) {
        guard polishHotkeyLabels.count > 1 else { return }
        var codes = settings.polishHotkeyKeyCodes
        var labels = settings.polishHotkeyLabels
        codes.remove(at: index)
        labels.remove(at: index)
        settings.polishHotkeyKeyCodes = codes
        settings.polishHotkeyLabels = labels
        settings.save()
        polishHotkeyLabels = labels
    }

    func startCapturingPolishHotkey() {
        if isCapturingPolishShortcut {
            stopCapturingPolishHotkey()
            return
        }
        isCapturingPolishShortcut = true

        polishShortcutMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { [weak self] event in
            guard let self = self else { return event }
            let keycode = event.keyCode

            // For flagsChanged, only capture on press, not release
            if event.type == .flagsChanged {
                guard HotkeyListener.isModifierDown(keycode: keycode, flags: event.modifierFlags) else { return nil }
            }

            let label: String
            if let knownLabel = HotkeyListener.keycodeLabels[keycode] {
                label = knownLabel
            } else {
                label = (event.charactersIgnoringModifiers ?? "?").uppercased()
            }

            // Don't add if already in the list
            guard !self.settings.polishHotkeyKeyCodes.contains(keycode) else {
                self.stopCapturingPolishHotkey()
                return nil
            }

            var codes = self.settings.polishHotkeyKeyCodes
            var labels = self.settings.polishHotkeyLabels
            codes.append(keycode)
            labels.append(label)
            self.settings.polishHotkeyKeyCodes = codes
            self.settings.polishHotkeyLabels = labels
            self.settings.save()
            self.polishHotkeyLabels = labels
            self.stopCapturingPolishHotkey()
            return nil
        }
    }

    private func stopCapturingPolishHotkey() {
        isCapturingPolishShortcut = false
        if let monitor = polishShortcutMonitor {
            NSEvent.removeMonitor(monitor)
            polishShortcutMonitor = nil
        }
    }

    func updateLaunchAgent() {
        let launchAgentsDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents")
        let plistPath = launchAgentsDir.appendingPathComponent("com.wisprlightning.app.plist")

        if settings.launchAtLogin {
            try? FileManager.default.createDirectory(at: launchAgentsDir, withIntermediateDirectories: true)
            let execPath = Bundle.main.executablePath ?? "/Applications/Wispr Lightning.app/Contents/MacOS/WisprLightning"
            let plist: [String: Any] = [
                "Label": "com.wisprlightning.app",
                "ProgramArguments": [execPath],
                "RunAtLoad": true,
                "KeepAlive": false,
            ]
            let data = try? PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
            try? data?.write(to: plistPath)
        } else {
            try? FileManager.default.removeItem(at: plistPath)
        }
    }
}
