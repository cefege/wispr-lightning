import AppKit
import SwiftUI

// MARK: - Tab Enum

enum MainTab: String, CaseIterable, Identifiable {
    case history
    case settings
    case account

    var id: String { rawValue }

    var title: String {
        switch self {
        case .history: return "History"
        case .settings: return "Settings"
        case .account: return "Account"
        }
    }

    var icon: String {
        switch self {
        case .history: return "clock"
        case .settings: return "gearshape"
        case .account: return "person.crop.circle"
        }
    }
}

// MARK: - Main View

struct MainView: View {
    @ObservedObject var historyVM: HistoryViewModel
    @ObservedObject var settingsVM: SettingsViewModel
    let session: Session
    @State private var selectedTab: MainTab = .history

    var body: some View {
        TabView(selection: $selectedTab) {
            HistoryView(vm: historyVM)
                .tabItem { Label(MainTab.history.title, systemImage: MainTab.history.icon) }
                .tag(MainTab.history)

            AllSettingsView(vm: settingsVM)
                .tabItem { Label(MainTab.settings.title, systemImage: MainTab.settings.icon) }
                .tag(MainTab.settings)

            AccountView(session: session)
                .tabItem { Label(MainTab.account.title, systemImage: MainTab.account.icon) }
                .tag(MainTab.account)
        }
    }
}

// MARK: - Account View

struct AccountView: View {
    let session: Session
    @State private var isSignedIn: Bool = false
    @State private var email: String = ""

    var body: some View {
        VStack(spacing: Theme.Spacing.xlarge) {
            Spacer()

            if isSignedIn {
                Image(systemName: "person.crop.circle.fill")
                    .font(.system(size: 48))
                    .foregroundStyle(.secondary)

                Text(email)
                    .font(.title3)

                Button("Sign Out") {
                    session.clear()
                    NotificationCenter.default.post(name: .sessionChanged, object: nil)
                }
                .controlSize(.large)
            } else {
                Image(systemName: "person.crop.circle.badge.questionmark")
                    .font(.system(size: 48))
                    .foregroundStyle(.secondary)

                Text("Not signed in")
                    .font(.title3)
                    .foregroundStyle(.secondary)

                Button("Sign In with Google") {
                    AuthService.signInWithBrowser()
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
        email = session.userEmail ?? ""
    }
}

// MARK: - Window Controller

class MainWindow {
    private var window: NSWindow?
    private let session: Session
    private let settings: AppSettings
    private let historyStore: HistoryStore
    private var historyVM: HistoryViewModel?
    private var settingsVM: SettingsViewModel?

    init(session: Session, settings: AppSettings, historyStore: HistoryStore) {
        self.session = session
        self.settings = settings
        self.historyStore = historyStore
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
        self.historyVM = hvm
        self.settingsVM = svm

        let mainView = MainView(historyVM: hvm, settingsVM: svm, session: session)
        let hostingView = NSHostingView(rootView: mainView)

        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 540, height: 520),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        w.title = "Wispr Lightning"
        w.center()
        w.isReleasedWhenClosed = false
        w.minSize = NSSize(width: 480, height: 400)
        w.contentView = hostingView

        self.window = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}
