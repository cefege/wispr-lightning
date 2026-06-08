import AppKit
import AVFoundation
import ApplicationServices
import Combine
import CoreGraphics
import IOKit.hid

/// Single source of truth for the four TCC permissions Lightning depends on.
/// `OnboardingWindow` uses these to gate "Get Started"; `Settings → Privacy`
/// surfaces the same status. Kept as pure statics + a snapshot helper so
/// the gating decision is unit-testable without touching real TCC state.

enum PermissionStatus: Equatable {
    case granted
    case notDetermined
    case denied
}

enum Permission: CaseIterable {
    case microphone
    case inputMonitoring
    case accessibility
    case screenRecording

    var title: String {
        switch self {
        case .microphone:       return "Microphone"
        case .inputMonitoring:  return "Input Monitoring"
        case .accessibility:    return "Accessibility"
        case .screenRecording:  return "Screen Recording"
        }
    }

    var rationale: String {
        switch self {
        case .microphone:
            return "Record your voice for dictation."
        case .inputMonitoring:
            return "Listen for your global push-to-talk hotkey when other apps are focused."
        case .accessibility:
            return "Paste transcripts at the cursor and type characters in Natural Mode."
        case .screenRecording:
            return "Optional — read on-screen text as transcription context. macOS will quit Wispr Lightning after you grant this; relaunch from /Applications."
        }
    }

    /// Required permissions block the "Get Started" button. Screen Recording is
    /// optional — useful only when `settings.useScreenContext` is on.
    var isRequired: Bool {
        switch self {
        case .microphone, .inputMonitoring, .accessibility: return true
        case .screenRecording: return false
        }
    }

    var systemSettingsURL: URL {
        let path: String
        switch self {
        case .microphone:
            path = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        case .inputMonitoring:
            path = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        case .accessibility:
            path = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        case .screenRecording:
            path = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        }
        return URL(string: path)!
    }
}

enum PermissionsManager {
    static func status(_ p: Permission) -> PermissionStatus {
        switch p {
        case .microphone:
            switch AVCaptureDevice.authorizationStatus(for: .audio) {
            case .authorized: return .granted
            case .notDetermined: return .notDetermined
            case .denied, .restricted: return .denied
            @unknown default: return .denied
            }
        case .inputMonitoring:
            // macOS 10.15+: IOHID-level TCC for global key listening.
            // kIOHIDAccessTypeGranted / Denied / Unknown.
            let access = IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)
            switch access {
            case kIOHIDAccessTypeGranted: return .granted
            case kIOHIDAccessTypeDenied:  return .denied
            default:                       return .notDetermined
            }
        case .accessibility:
            // macOS conflates not-asked and denied here. Treating both as
            // "needs action" is fine for the onboarding gate.
            return AXIsProcessTrusted() ? .granted : .notDetermined
        case .screenRecording:
            return CGPreflightScreenCaptureAccess() ? .granted : .notDetermined
        }
    }

    static func allRequiredGranted() -> Bool {
        var snapshot: [Permission: PermissionStatus] = [:]
        for p in Permission.allCases { snapshot[p] = status(p) }
        return allRequiredGranted(from: snapshot)
    }

    static func allRequiredGranted(from snapshot: [Permission: PermissionStatus]) -> Bool {
        Permission.allCases.filter { $0.isRequired }.allSatisfy { snapshot[$0] == .granted }
    }

    /// One-call entry point used by the UI: fires the right prompt for the
    /// permission, or opens System Settings if the user has already denied
    /// (the OS won't re-prompt once denied).
    static func requestAccess(_ p: Permission, currentStatus: PermissionStatus) {
        if currentStatus == .denied {
            openSystemSettings(p)
            return
        }
        switch p {
        case .microphone:
            AVCaptureDevice.requestAccess(for: .audio) { _ in }
        case .inputMonitoring:
            _ = IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)
            openSystemSettings(.inputMonitoring)
        case .accessibility:
            _ = AXIsProcessTrustedWithOptions(
                [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary
            )
            openSystemSettings(.accessibility)
        case .screenRecording:
            _ = CGRequestScreenCaptureAccess()
        }
    }

    static func openSystemSettings(_ p: Permission) {
        NSWorkspace.shared.open(p.systemSettingsURL)
    }
}

/// Observable poller used by the onboarding window and (later) Settings.
/// macOS doesn't notify on TCC grants, so we re-read every second and publish
/// only when the snapshot changes.
final class PermissionStatusPoller: ObservableObject {
    @Published private(set) var statuses: [Permission: PermissionStatus] = [:]
    private var timer: Timer?

    init() {
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            self?.refresh()
        }
    }

    deinit { timer?.invalidate() }

    func refresh() {
        var next: [Permission: PermissionStatus] = [:]
        for p in Permission.allCases { next[p] = PermissionsManager.status(p) }
        if next != statuses { statuses = next }
    }

    var allRequiredGranted: Bool {
        PermissionsManager.allRequiredGranted(from: statuses)
    }
}
