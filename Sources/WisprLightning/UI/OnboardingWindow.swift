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

    private var policyBeforeOpen: NSApplication.ActivationPolicy?

    private func promoteToRegular() {
        if policyBeforeOpen == nil {
            policyBeforeOpen = NSApp.activationPolicy()
        }
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

    func show() {
        promoteToRegular()
        if let w = window {
            w.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        let view = OnboardingView(settings: settings, onContinue: { [weak self] in
            guard let self else { return }
            self.settings.didCompleteOnboarding = true
            self.settings.save()
            self.window?.close()
            self.stopObservingActivation()
            self.restorePolicy()
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

private enum OnboardingStep: Int { case permissions = 0, mic, vendor }

private struct OnboardingView: View {
    @StateObject private var poller = PermissionStatusPoller()
    @State private var step: OnboardingStep = .permissions
    let settings: AppSettings
    let onContinue: () -> Void

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: "bolt.fill")
                .font(.system(size: 48))
                .foregroundStyle(LinearGradient(
                    colors: [Color.yellow, Color.orange],
                    startPoint: .top, endPoint: .bottom
                ))
                .frame(width: 60, height: 60)
            Text("Welcome to Wispr Lightning")
                .font(.title.bold())

            // Step dots
            HStack(spacing: 8) {
                ForEach(0..<3, id: \.self) { i in
                    Circle()
                        .fill(i == step.rawValue ? Color.orange : Color.secondary.opacity(0.3))
                        .frame(width: 8, height: 8)
                }
            }

            Group {
                switch step {
                case .permissions: permissionsPage
                case .mic:         micTestPage
                case .vendor:      vendorPickPage
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            footer
        }
        .padding(.top, 20)
        .frame(width: 520, height: 640)
    }

    // MARK: - Pages

    @ViewBuilder
    private var permissionsPage: some View {
        VStack(spacing: 14) {
            Text("Grant the permissions Lightning needs to listen for your hotkey and type transcripts at the cursor.")
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
        }
    }

    @ViewBuilder
    private var micTestPage: some View {
        VStack(spacing: 14) {
            Text("Test your microphone")
                .font(.title3.weight(.semibold))
            Text("Say something — you should see the bar move. If it stays flat, switch to a different input device or check your system input settings.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 24)

            MicTestView(settings: settings)
                .padding(.horizontal, 20)
        }
    }

    @ViewBuilder
    private var vendorPickPage: some View {
        VendorPickView()
    }

    // MARK: - Footer

    @ViewBuilder
    private var footer: some View {
        let nextEnabled: Bool = {
            switch step {
            case .permissions: return poller.allRequiredGranted
            case .mic, .vendor: return true
            }
        }()
        let nextLabel: String = {
            switch step {
            case .permissions: return poller.allRequiredGranted ? "Continue" : "Continue (some permissions missing)"
            case .mic:         return "Continue"
            case .vendor:      return "Finish setup"
            }
        }()

        VStack(spacing: 6) {
            HStack {
                if step != .permissions {
                    Button("Back") {
                        if let prev = OnboardingStep(rawValue: step.rawValue - 1) { step = prev }
                    }
                }
                Spacer()
                Button {
                    if let next = OnboardingStep(rawValue: step.rawValue + 1) {
                        step = next
                    } else {
                        onContinue()
                    }
                } label: {
                    Text(nextLabel)
                        .frame(minWidth: 140)
                        .padding(.vertical, 2)
                }
                .keyboardShortcut(.defaultAction)
                .disabled(step == .permissions && !nextEnabled)
            }
            .padding(.horizontal, 20)

            if step == .permissions && !poller.allRequiredGranted {
                Text("Grant Microphone, Input Monitoring, and Accessibility to continue.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.bottom, 16)
    }
}

/// Subscribes to AudioRecorder.onLevelUpdate during onboarding so the user
/// sees a live RMS bar before a real dictation. Starts/stops with the view.
/// Bails out when a real dictation is in flight (AudioRecorder.isAnyActive)
/// — opening a second AVAudioEngine against the same input either shows a
/// flat meter or steals audio from the live recording.
private struct MicTestView: View {
    let settings: AppSettings
    @State private var level: Float = 0
    @State private var recorder: AudioRecorder? = nil
    @State private var conflict: Bool = false

    var body: some View {
        VStack(spacing: 12) {
            if conflict {
                HStack(spacing: 8) {
                    Image(systemName: "waveform.badge.exclamationmark")
                        .foregroundStyle(.orange)
                    Text("A dictation is in progress — skip this step and test the mic after.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                .padding(12)
                .background(Color.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
            } else {
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(Color.secondary.opacity(0.15))
                        .frame(height: 24)
                    GeometryReader { geo in
                        Capsule()
                            .fill(LinearGradient(colors: [.green, .yellow, .red], startPoint: .leading, endPoint: .trailing))
                            .frame(width: geo.size.width * CGFloat(min(1, level * 1.6)), height: 24)
                            .animation(.linear(duration: 0.05), value: level)
                    }
                    .frame(height: 24)
                }
                .frame(maxWidth: .infinity)

                Text(level < 0.02
                     ? "No signal yet — try speaking, or check your input device."
                     : "Looks good — Lightning hears you.")
                    .font(.caption)
                    .foregroundColor(level < 0.02 ? .secondary : .green)
            }
        }
        .padding(.horizontal, 4)
        .onAppear { start() }
        .onDisappear { stop() }
    }

    private func start() {
        if AudioRecorder.isAnyActive {
            conflict = true
            return
        }
        // Use the LIVE settings instance from the onboarding controller so a
        // mic device picked here can't drift from what the rest of the app
        // sees. (AppSettings is effectively a singleton — a second .load()
        // would create a parallel instance that doesn't observe future
        // settingsChanged notifications.)
        let r = AudioRecorder(settings: settings)
        r.onLevelUpdate = { lvl in
            DispatchQueue.main.async { level = lvl }
        }
        recorder = r
        _ = r.start()
    }

    private func stop() {
        recorder?.onLevelUpdate = nil
        _ = recorder?.stop()
        recorder?.cleanup()
        recorder = nil
    }
}

/// Lets the user pick a primary transcription vendor at the end of onboarding
/// and jump straight to its auth surface in Settings. Just sets activeVendor
/// — the actual sign-in happens in Settings → Accounts.
private struct VendorPickView: View {
    @State private var selected: String = DictationVendor.wisprFlow.rawValue

    var body: some View {
        VStack(spacing: 14) {
            Text("Pick a transcription provider")
                .font(.title3.weight(.semibold))
            Text("You can change this any time in Settings → Provider. Add fallbacks there too.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 24)

            VStack(spacing: 10) {
                ForEach(DictationVendor.allCases, id: \.rawValue) { vendor in
                    VendorChoice(vendor: vendor, selected: $selected)
                }
            }
            .padding(.horizontal, 20)
            .onChange(of: selected) { newValue in
                let s = AppSettings.load()
                s.activeVendor = newValue
                s.save()
            }
        }
    }
}

private struct VendorChoice: View {
    let vendor: DictationVendor
    @Binding var selected: String

    private var isSelected: Bool { selected == vendor.rawValue }

    var body: some View {
        Button { selected = vendor.rawValue } label: {
            HStack(spacing: 12) {
                Image(systemName: isSelected ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(isSelected ? Color.accentColor : Color.secondary)
                    .font(.title2)
                VStack(alignment: .leading, spacing: 2) {
                    Text(vendor.displayName).font(.body.bold())
                    Text(rationale).font(.footnote).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer()
            }
            .padding(12)
            .background(isSelected
                        ? Color.accentColor.opacity(0.12)
                        : Color(NSColor.controlBackgroundColor))
            .cornerRadius(8)
        }
        .buttonStyle(.plain)
    }

    private var rationale: String {
        switch vendor {
        case .wisprFlow:
            return "Sign in with your Wispr Flow account. Best transcription quality plus the Polish feature."
        case .openRouter:
            return "BYO API key. Pay OpenRouter directly for any audio-input model (Gemini, Whisper, etc.). Set up in Accounts."
        case .claudeVoice:
            return "Uses the `claude` CLI's stored credentials. Live streaming. Run `claude /login` once if you haven't."
        }
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
