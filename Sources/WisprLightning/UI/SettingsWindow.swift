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
    case general, dictation, polish
    case history, dictionary, notes
    case privacy, system

    var id: String { rawValue }

    var title: String {
        switch self {
        case .general: return "General"
        case .dictation: return "Dictation"
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
    @State private var isSignedIn = false
    @State private var email = ""
    @State private var displayName = ""
    @State private var avatarURL: String? = nil
    @State private var selectedSection: SettingsSection = .general

    init(vm: SettingsViewModel, session: Session, historyStore: HistoryStore, dictionaryStore: DictionaryStore, notesStore: NotesStore) {
        self.vm = vm
        self.session = session
        self._historyVM = StateObject(wrappedValue: HistoryViewModel(historyStore: historyStore))
        self._dictionaryVM = StateObject(wrappedValue: DictionaryViewModel(dictionaryStore: dictionaryStore))
        self._notesVM = StateObject(wrappedValue: NotesViewModel(notesStore: notesStore))
    }

    private static let settingsGroup: [SettingsSection] = [.general, .dictation, .polish]
    private static let dataGroup: [SettingsSection] = [.history, .dictionary, .notes]
    private static let systemGroup: [SettingsSection] = [.privacy, .system]

    var body: some View {
        NavigationSplitView {
            List(selection: $selectedSection) {
                Section {
                    ForEach(Self.settingsGroup) { section in
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
                                AccountSection(isSignedIn: isSignedIn, displayName: displayName, email: email, avatarURL: avatarURL, session: session)
                                Divider()
                                ShortcutsDetail(vm: vm)
                                Divider()
                                MicrophoneDetail(vm: vm)
                                Divider()
                                LanguagesDetail(vm: vm)
                            case .dictation:
                                DictationDetail(vm: vm)
                                Divider()
                                PersonalizationDetail(vm: vm)
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
        .onAppear { refreshAccount() }
        .onReceive(NotificationCenter.default.publisher(for: .sessionChanged)) { _ in
            refreshAccount()
        }
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

    private func refreshAccount() {
        isSignedIn = session.isValid
        email = session.userEmail ?? ""
        avatarURL = session.avatarURL
        let first = session.userFirstName ?? ""
        let last = session.userLastName ?? ""
        let full = [first, last].filter { !$0.isEmpty }.joined(separator: " ")
        displayName = full.isEmpty ? email : full
    }
}

// MARK: - Settings Window Controller

class SettingsWindowController {
    private var window: NSWindow?
    private var settingsVM: SettingsViewModel?
    private let settings: AppSettings
    private let session: Session
    private let historyStore: HistoryStore
    private let dictionaryStore: DictionaryStore
    private let notesStore: NotesStore

    init(settings: AppSettings, session: Session, historyStore: HistoryStore, dictionaryStore: DictionaryStore, notesStore: NotesStore) {
        self.settings = settings
        self.session = session
        self.historyStore = historyStore
        self.dictionaryStore = dictionaryStore
        self.notesStore = notesStore
    }

    func showWindow() {
        if let window = window {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
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

        self.window = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}

// MARK: - Account Section

private struct AccountSection: View {
    let isSignedIn: Bool
    let displayName: String
    let email: String
    let avatarURL: String?
    let session: Session

    var body: some View {
        Text("Account")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
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
                                Text(displayName)
                                    .font(.body.weight(.medium))
                            }
                            Text(email)
                                .font(.caption)
                                .foregroundStyle(.secondary)
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
                        Text("Not signed in")
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button("Sign In with Google") {
                            AuthService.signInWithBrowser()
                        }
                        .controlSize(.small)
                    }
                }
            }
            .padding(Theme.Spacing.medium)
        }
    }
}

// MARK: - Shortcuts Detail

private struct ShortcutsDetail: View {
    @ObservedObject var vm: SettingsViewModel

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
            }
            .padding(Theme.Spacing.medium)
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
            }
            .padding(Theme.Spacing.medium)
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
    @Published var hotkeyLabels: [String] = []
    @Published var selectedMicUID: String?
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

    // Sound Packs
    @Published var selectedSoundPack: String?
    @Published var availableSoundPacks: [String] = []

    // Debug
    @Published var verboseLogging: Bool

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
        self.hotkeyLabels = settings.hotkeyLabels.isEmpty ? [settings.hotkeyLabel] : settings.hotkeyLabels

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

        // Sound Packs
        self.selectedSoundPack = settings.selectedSoundPack

        // Debug
        self.verboseLogging = settings.verboseLogging

        refreshMicDevices()
        availableSoundPacks = SoundManager.availablePacks()
    }

    func refreshMicDevices() {
        micDevices = AudioRecorder.listInputDevices()
    }

    func saveMicSelection() {
        if let uid = selectedMicUID {
            settings.micDeviceUID = uid
            settings.micDeviceName = micDevices.first(where: { $0.uid == uid })?.name
        } else {
            settings.micDeviceUID = nil
            settings.micDeviceName = nil
        }
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
        settings.hotkeyKeyCode = codes[0]
        settings.hotkeyLabel = labels[0]
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
            self.settings.hotkeyKeyCode = codes[0]
            self.settings.hotkeyLabel = labels[0]
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
