import AppKit
import SwiftUI

// MARK: - All Settings View (stacked vertically for tab embedding)

struct AllSettingsView: View {
    @ObservedObject var vm: SettingsViewModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.large) {
                ShortcutsDetail(vm: vm)
                Divider()
                MicrophoneDetail(vm: vm)
                Divider()
                LanguagesDetail(vm: vm)
                Divider()
                SystemDetail(vm: vm)
            }
            .padding(Theme.Spacing.xlarge)
            .frame(maxWidth: .infinity, alignment: .leading)
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

    var body: some View {
        Text("Dictation Language")
            .font(.title3.weight(.semibold))

        GroupBox {
            VStack(alignment: .leading, spacing: Theme.Spacing.medium) {
                Picker("Language", selection: $vm.selectedLanguage) {
                    ForEach(SettingsViewModel.languages, id: \.code) { lang in
                        Text(lang.name).tag(lang.code)
                    }
                }
                .labelsHidden()
                .onChange(of: vm.selectedLanguage) { _ in vm.saveLanguage() }

                Text("Setting a language improves accuracy.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
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
                Toggle("Launch at login", isOn: $vm.launchAtLogin)
                    .onChange(of: vm.launchAtLogin) { _ in vm.saveSystemSettings(); vm.updateLaunchAgent() }

                Toggle("Show in Dock", isOn: $vm.showInDock)
                    .onChange(of: vm.showInDock) { _ in
                        vm.saveSystemSettings()
                        NSApp.setActivationPolicy(vm.showInDock ? .regular : .accessory)
                    }

                Toggle("Sound effects", isOn: $vm.enableSounds)
                    .onChange(of: vm.enableSounds) { _ in vm.saveSystemSettings() }

                Toggle("Mute music while dictating", isOn: $vm.muteMusic)
                    .onChange(of: vm.muteMusic) { _ in vm.saveSystemSettings() }
            }
            .padding(Theme.Spacing.medium)
        }

        Divider()

        Text("Wispr Lite v1.0.0")
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
    @Published var selectedLanguage: String
    @Published var launchAtLogin: Bool
    @Published var showInDock: Bool
    @Published var enableSounds: Bool
    @Published var muteMusic: Bool
    @Published var micDevices: [(uid: String, name: String)] = []

    private var shortcutMonitor: Any?

    struct Language {
        let code: String
        let name: String
    }

    static let languages: [Language] = [
        .init(code: "en", name: "English"),
        .init(code: "en-GB", name: "English (UK)"),
        .init(code: "es", name: "Spanish"),
        .init(code: "de", name: "German"),
        .init(code: "fr", name: "French"),
        .init(code: "it", name: "Italian"),
        .init(code: "pt", name: "Portuguese"),
        .init(code: "ro", name: "Romanian"),
        .init(code: "nl", name: "Dutch"),
        .init(code: "ja", name: "Japanese"),
        .init(code: "ko", name: "Korean"),
        .init(code: "zh", name: "Chinese"),
    ]

    init(settings: AppSettings) {
        self.settings = settings
        self.selectedMicUID = settings.micDeviceUID
        self.selectedLanguage = settings.languages.first ?? "en"
        self.launchAtLogin = settings.launchAtLogin
        self.showInDock = settings.showInDock
        self.enableSounds = settings.enableSounds
        self.muteMusic = settings.muteMusic
        self.hotkeyLabels = settings.hotkeyLabels.isEmpty ? [settings.hotkeyLabel] : settings.hotkeyLabels
        refreshMicDevices()
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

    func saveLanguage() {
        settings.languages = [selectedLanguage]
        settings.save()
    }

    func saveSystemSettings() {
        settings.launchAtLogin = launchAtLogin
        settings.showInDock = showInDock
        settings.enableSounds = enableSounds
        settings.muteMusic = muteMusic
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

    func updateLaunchAgent() {
        let launchAgentsDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents")
        let plistPath = launchAgentsDir.appendingPathComponent("com.wisprlite.app.plist")

        if settings.launchAtLogin {
            try? FileManager.default.createDirectory(at: launchAgentsDir, withIntermediateDirectories: true)
            let execPath = Bundle.main.executablePath ?? "/Applications/Wispr Lite.app/Contents/MacOS/WisprLite"
            let plist: [String: Any] = [
                "Label": "com.wisprlite.app",
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
