import AppKit
import SwiftUI

// MARK: - Sidebar Navigation

enum SidebarItem: String, CaseIterable, Identifiable {
    case home
    case history
    case dictionary
    case notes

    var id: String { rawValue }

    var title: String {
        switch self {
        case .home: return "Home"
        case .history: return "History"
        case .dictionary: return "Dictionary"
        case .notes: return "Notes"
        }
    }

    var icon: String {
        switch self {
        case .home: return "bolt.fill"
        case .history: return "clock"
        case .dictionary: return "character.book.closed"
        case .notes: return "note.text"
        }
    }
}

// MARK: - Main View

struct MainView: View {
    @ObservedObject var historyVM: HistoryViewModel
    @ObservedObject var settingsVM: SettingsViewModel
    @ObservedObject var dictionaryVM: DictionaryViewModel
    @ObservedObject var notesVM: NotesViewModel
    let session: Session
    let historyStore: HistoryStore
    @State private var selectedItem: SidebarItem = .home

    var body: some View {
        NavigationSplitView {
            List(SidebarItem.allCases, selection: $selectedItem) { item in
                Label(item.title, systemImage: item.icon)
                    .tag(item)
            }
            .listStyle(.sidebar)
            .navigationSplitViewColumnWidth(180)
        } detail: {
            switch selectedItem {
            case .home:
                HomeView(session: session, historyStore: historyStore, settingsVM: settingsVM)
            case .history:
                HistoryView(vm: historyVM)
            case .dictionary:
                DictionaryView(vm: dictionaryVM)
            case .notes:
                NotesView(vm: notesVM)
            }
        }
    }
}

// MARK: - Home View

struct HomeView: View {
    let session: Session
    let historyStore: HistoryStore
    @ObservedObject var settingsVM: SettingsViewModel
    @State private var isSignedIn = false
    @State private var isAccessibilityTrusted = false
    @State private var todayDictations = 0
    @State private var todayWords = 0

    private var statusInfo: (label: String, color: Color, icon: String) {
        if !isSignedIn {
            return ("Sign in required", .red, "xmark.circle.fill")
        }
        if !isAccessibilityTrusted {
            return ("Permissions needed", .orange, "exclamationmark.triangle.fill")
        }
        return ("Ready to dictate", .green, "checkmark.circle.fill")
    }

    var body: some View {
        VStack(spacing: Theme.Spacing.xlarge) {
            Spacer()

            // Hero icon
            Image(systemName: "bolt.fill")
                .font(.system(size: 48))
                .foregroundStyle(.tint)

            // Status badge
            HStack(spacing: 6) {
                Image(systemName: statusInfo.icon)
                    .foregroundStyle(statusInfo.color)
                Text(statusInfo.label)
                    .font(.headline)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(statusInfo.color.opacity(0.1))
            .cornerRadius(8)

            // Hotkey callout
            if let firstLabel = settingsVM.hotkeyLabels.first {
                VStack(spacing: Theme.Spacing.small) {
                    KeyCapView(label: firstLabel)
                    Text("Hold to dictate")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }

            // Today's stats
            if todayDictations > 0 {
                Text("\(todayDictations) dictation\(todayDictations == 1 ? "" : "s") · \(todayWords) words today")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else {
                Text("No dictations yet — try it!")
                    .font(.subheadline)
                    .foregroundStyle(.tertiary)
            }

            // Sign in button (if needed)
            if !isSignedIn {
                Button("Sign In with Google") {
                    AuthService.signInWithBrowser()
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
            }

            // Accessibility permission button (if needed)
            if isSignedIn && !isAccessibilityTrusted {
                Button("Grant Accessibility Permission") {
                    AXIsProcessTrustedWithOptions(
                        [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary
                    )
                }
                .controlSize(.large)
            }

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onAppear { refreshState() }
        .onReceive(NotificationCenter.default.publisher(for: .sessionChanged)) { _ in
            refreshState()
        }
    }

    private func refreshState() {
        isSignedIn = session.isValid
        isAccessibilityTrusted = AXIsProcessTrusted()
        let stats = historyStore.todayStats()
        todayDictations = stats.dictations
        todayWords = stats.words
    }
}

// MARK: - Window Controller

class MainWindow {
    private var window: NSWindow?
    private let session: Session
    private let settings: AppSettings
    private let historyStore: HistoryStore
    private let dictionaryStore: DictionaryStore
    private let notesStore: NotesStore
    private var historyVM: HistoryViewModel?
    private var settingsVM: SettingsViewModel?
    private var dictionaryVM: DictionaryViewModel?
    private var notesVM: NotesViewModel?

    init(session: Session, settings: AppSettings, historyStore: HistoryStore, dictionaryStore: DictionaryStore, notesStore: NotesStore) {
        self.session = session
        self.settings = settings
        self.historyStore = historyStore
        self.dictionaryStore = dictionaryStore
        self.notesStore = notesStore
    }

    func showWindow() {
        if let window = window {
            historyVM?.refresh()
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }

        let hvm = HistoryViewModel(historyStore: historyStore)
        let svm = SettingsViewModel(settings: settings)
        let dvm = DictionaryViewModel(dictionaryStore: dictionaryStore)
        let nvm = NotesViewModel(notesStore: notesStore)
        self.historyVM = hvm
        self.settingsVM = svm
        self.dictionaryVM = dvm
        self.notesVM = nvm

        let mainView = MainView(historyVM: hvm, settingsVM: svm, dictionaryVM: dvm, notesVM: nvm, session: session, historyStore: historyStore)
        let hostingView = NSHostingView(rootView: mainView)

        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 640, height: 480),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        w.title = "Wispr Lightning"
        w.center()
        w.setFrameAutosaveName("MainWindow")
        w.isReleasedWhenClosed = false
        w.minSize = NSSize(width: 560, height: 400)
        w.contentView = hostingView

        self.window = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}
