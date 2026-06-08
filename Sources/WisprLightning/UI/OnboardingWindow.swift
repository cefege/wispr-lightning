import AppKit
import SwiftUI

/// First-run permissions wizard. PermissionStatusPoller re-reads TCC state
/// every 1s (macOS doesn't notify on grant) and the "Get Started" button is
/// gated on the required permissions (Microphone + Input Monitoring +
/// Accessibility). Screen Recording is marked Optional and does not block.
final class OnboardingWindowController {
    private var window: NSWindow?
    private var becomeActiveObserver: NSObjectProtocol?
    private let settings: AppSettings
    private let onCompleted: () -> Void

    init(settings: AppSettings, onCompleted: @escaping () -> Void) {
        self.settings = settings
        self.onCompleted = onCompleted
    }

    deinit {
        if let observer = becomeActiveObserver {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    func show() {
        if let w = window {
            w.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        let view = OnboardingView(onContinue: { [weak self] in
            guard let self else { return }
            self.settings.didCompleteOnboarding = true
            self.settings.save()
            self.window?.close()
            self.stopObservingActivation()
            self.onCompleted()
        })
        let hosting = NSHostingController(rootView: view)
        let win = NSWindow(contentViewController: hosting)
        win.title = "Welcome to Wispr Lightning"
        win.setContentSize(NSSize(width: 480, height: 600))
        win.styleMask = [.titled, .closable, .miniaturizable]
        win.center()
        win.isReleasedWhenClosed = false
        self.window = win
        win.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        startObservingActivation()
    }

    /// Granting a permission yanks focus to the OS prompt / System Settings.
    /// When the user comes back, our window can get buried — re-raise it on
    /// every reactivation until onboarding is dismissed.
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
}

private struct OnboardingView: View {
    @StateObject private var poller = PermissionStatusPoller()
    let onContinue: () -> Void

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: "bolt.fill")
                .font(.system(size: 56))
                .foregroundStyle(LinearGradient(
                    colors: [Color.yellow, Color.orange],
                    startPoint: .top, endPoint: .bottom
                ))
                .frame(width: 72, height: 72)
            Text("Welcome to Wispr Lightning")
                .font(.title.bold())
            Text("Dictate anywhere on your Mac. Grant these permissions to get started.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 24)

            VStack(spacing: 10) {
                ForEach(Permission.allCases, id: \.self) { p in
                    PermissionRow(
                        permission: p,
                        status: poller.statuses[p] ?? .notDetermined
                    )
                }
            }
            .padding(.horizontal, 20)

            Spacer(minLength: 0)

            VStack(spacing: 4) {
                Button(action: onContinue) {
                    Text(poller.allRequiredGranted ? "Get Started" : "Continue Anyway")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 4)
                }
                .keyboardShortcut(.defaultAction)

                if !poller.allRequiredGranted {
                    Text("Grant Microphone, Input Monitoring, and Accessibility to dictate.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 20)
            .padding(.bottom, 16)
        }
        .padding(.top, 24)
        .frame(width: 480, height: 600)
    }
}

private struct PermissionRow: View {
    let permission: Permission
    let status: PermissionStatus

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: status.iconName)
                .foregroundStyle(status.iconColor)
                .font(.title2)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(permission.title).font(.body.bold())
                    if !permission.isRequired {
                        Text("Optional")
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.secondary.opacity(0.18), in: Capsule())
                            .foregroundStyle(.secondary)
                    }
                }
                Text(permission.rationale)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            if status == .granted {
                Text("Granted")
                    .font(.caption)
                    .foregroundStyle(.green)
            } else {
                Button(status == .denied ? "Open Settings" : "Grant") {
                    PermissionsManager.requestAccess(permission, currentStatus: status)
                }
                .controlSize(.small)
            }
        }
        .padding(12)
        .background(Color(NSColor.controlBackgroundColor))
        .cornerRadius(8)
    }
}

private extension PermissionStatus {
    var iconName: String {
        switch self {
        case .granted: return "checkmark.circle.fill"
        case .notDetermined: return "exclamationmark.circle.fill"
        case .denied: return "xmark.circle.fill"
        }
    }
    var iconColor: Color {
        switch self {
        case .granted: return .green
        case .notDetermined: return .orange
        case .denied: return .red
        }
    }
}
