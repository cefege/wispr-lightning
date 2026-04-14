import AppKit
import CoreGraphics

class HotkeyListener {
    private let settings: AppSettings
    private let onPress: () -> Void
    private let onRelease: () -> Void
    var onPolishPress: (() -> Void)?
    private var keyDown = false
    private var activeKeyCode: UInt16? // which hotkey triggered the current recording
    private var monitors: [Any] = []
    private var _hotkeySet: Set<UInt16> = []
    private var _polishKeyCodes: Set<UInt16> = []
    private var lastPolishTriggerTime: Date?

    static let keycodeLabels: [UInt16: String] = [
        59: "Left Control",
        62: "Right Control",
        58: "Left Option",
        61: "Right Option",
        55: "Left Command",
        54: "Right Command",
        56: "Left Shift",
        60: "Right Shift",
        63: "Fn",
        36: "Return",
        49: "Space",
        53: "Escape",
        48: "Tab",
    ]

    /// All keycodes that independently trigger dictation
    private var hotkeySet: Set<UInt16> { _hotkeySet }

    private func rebuildHotkeySet() {
        let codes = settings.hotkeyKeyCodes
        _hotkeySet = codes.isEmpty ? [settings.hotkeyKeyCode] : Set(codes)
        _polishKeyCodes = Set(settings.polishHotkeyKeyCodes)
    }

    init(settings: AppSettings, onPress: @escaping () -> Void, onRelease: @escaping () -> Void) {
        self.settings = settings
        self.onPress = onPress
        self.onRelease = onRelease

        NotificationCenter.default.addObserver(
            forName: .settingsChanged, object: nil, queue: .main
        ) { [weak self] _ in
            self?.rebuildHotkeySet()
        }
    }

    /// User-controlled pause toggle. While true, all press handlers early-return.
    /// Persisted via AppSettings.hotkeyPaused so the state survives relaunches.
    var isPaused: Bool {
        get { settings.hotkeyPaused }
    }

    func setPaused(_ paused: Bool) {
        guard settings.hotkeyPaused != paused else { return }
        settings.hotkeyPaused = paused
        settings.save()
        wLog(paused ? "Hotkey paused" : "Hotkey resumed")
        // Reset key-down latch so a held key doesn't get "stuck" across pause toggles.
        keyDown = false
        activeKeyCode = nil
    }

    func start() {
        rebuildHotkeySet()
        installMonitors()
        // CGEventTap intentionally NOT installed: it sees events at a layer below
        // OS dispatch, so it fires even when Universal Control routes the keypress
        // to another Mac. NSEvent global monitors fire only when the event was
        // actually dispatched to an app on this Mac, which is the behavior we want.
        let labels = settings.hotkeyLabels.isEmpty ? [settings.hotkeyLabel] : settings.hotkeyLabels
        NSLog("Wispr Lightning: Hotkey listener active (press %@ to dictate)", labels.joined(separator: " or "))
    }

    func rebind(keyCode: UInt16) {
        removeMonitors()
        settings.hotkeyKeyCode = keyCode
        settings.hotkeyLabel = Self.keycodeLabels[keyCode] ?? "Key \(keyCode)"
        settings.hotkeyKeyCodes = [keyCode]
        settings.hotkeyLabels = [settings.hotkeyLabel]
        settings.save()
        rebuildHotkeySet()
        installMonitors()
    }

    private func installMonitors() {
        let flagsMonitor = NSEvent.addGlobalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            self?.handleFlagsChanged(event, path: "global-flags")
        }
        if let m = flagsMonitor { monitors.append(m) }

        let localFlags = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            self?.handleFlagsChanged(event, path: "local-flags")
            return event
        }
        if let m = localFlags { monitors.append(m) }

        let keyMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
            self?.handleKeyEvent(event)
        }
        if let m = keyMonitor { monitors.append(m) }
    }

    private func removeMonitors() {
        for monitor in monitors {
            NSEvent.removeMonitor(monitor)
        }
        monitors.removeAll()
        keyDown = false
        activeKeyCode = nil
    }

    private func triggerPolish() {
        let now = Date()
        if let last = lastPolishTriggerTime, now.timeIntervalSince(last) < 0.5 { return }
        lastPolishTriggerTime = now
        if settings.polishEnabled, let polishHandler = onPolishPress {
            DispatchQueue.main.async { polishHandler() }
        }
    }

    private func handleFlagsChanged(_ event: NSEvent, path: String) {
        let keycode = event.keyCode

        // Polish hotkey: modifier key used as standalone trigger
        if _polishKeyCodes.contains(keycode) && !hotkeySet.contains(keycode) {
            let pressed = Self.isModifierDown(keycode: keycode, flags: event.modifierFlags)
            let onScreen = isCursorOnLocalDisplay()
            let local = Self.isLocalHIDEvent(event)
            wLog("Hotkey[polish/\(path)] keycode=\(keycode) pressed=\(pressed) onScreen=\(onScreen) localHID=\(local) paused=\(isPaused)")
            if pressed && onScreen && local && !isPaused {
                triggerPolish()
            }
            return
        }

        guard hotkeySet.contains(keycode) else { return }

        let isPressed = Self.isModifierDown(keycode: keycode, flags: event.modifierFlags)
        let onScreen = isCursorOnLocalDisplay()
        let local = Self.isLocalHIDEvent(event)
        wLog("Hotkey[\(path)] keycode=\(keycode) pressed=\(isPressed) onScreen=\(onScreen) localHID=\(local) paused=\(isPaused) keyDown=\(keyDown)")

        if isPressed && !keyDown && onScreen && local && !isPaused {
            keyDown = true
            activeKeyCode = keycode
            onPress()
        } else if !isPressed && keyDown && activeKeyCode == keycode {
            keyDown = false
            activeKeyCode = nil
            onRelease()
        }
    }

    private func handleKeyEvent(_ event: NSEvent) {
        // Polish hotkey: regular key
        if event.type == .keyDown && _polishKeyCodes.contains(event.keyCode) && !isModifierKeycode(event.keyCode) {
            let onScreen = isCursorOnLocalDisplay()
            let local = Self.isLocalHIDEvent(event)
            wLog("Hotkey[polish/global-key] keycode=\(event.keyCode) onScreen=\(onScreen) localHID=\(local) paused=\(isPaused)")
            if onScreen && local && !isPaused { triggerPolish() }
            return
        }

        guard hotkeySet.contains(event.keyCode) else { return }
        guard !isModifierKeycode(event.keyCode) else { return }

        let onScreen = isCursorOnLocalDisplay()
        let local = Self.isLocalHIDEvent(event)
        wLog("Hotkey[global-key] keycode=\(event.keyCode) type=\(event.type.rawValue) onScreen=\(onScreen) localHID=\(local) paused=\(isPaused) keyDown=\(keyDown)")

        if event.type == .keyDown && !keyDown && onScreen && local && !isPaused {
            keyDown = true
            activeKeyCode = event.keyCode
            onPress()
        } else if event.type == .keyUp && keyDown && activeKeyCode == event.keyCode {
            keyDown = false
            activeKeyCode = nil
            onRelease()
        }
    }

    static func isModifierDown(keycode: UInt16, flags: NSEvent.ModifierFlags) -> Bool {
        switch keycode {
        case 59, 62: return flags.contains(.control)
        case 58, 61: return flags.contains(.option)
        case 55, 54: return flags.contains(.command)
        case 56, 60: return flags.contains(.shift)
        case 63: return flags.contains(.function)
        default: return false
        }
    }

    /// True when the event came from real local HID hardware (kernel-posted),
    /// false when it was synthesized by another process. Universal Control syncs
    /// modifier flag state across Macs by re-posting flagsChanged events from the
    /// UC daemon on the *other* Mac — those have a non-zero source PID and we
    /// reject them so the hotkey only fires on the Mac the user is physically on.
    private static func isLocalHIDEvent(_ cg: CGEvent) -> Bool {
        return cg.getIntegerValueField(.eventSourceUnixProcessID) == 0
    }

    private static func isLocalHIDEvent(_ event: NSEvent) -> Bool {
        guard let cg = event.cgEvent else { return true }
        return isLocalHIDEvent(cg)
    }

    private func isModifierKeycode(_ keycode: UInt16) -> Bool {
        return [59, 62, 58, 61, 55, 54, 56, 60, 63].contains(keycode)
    }

    func resetState() {
        keyDown = false
        activeKeyCode = nil
    }

    private func isCursorOnLocalDisplay() -> Bool {
        let mouseLocation = NSEvent.mouseLocation
        return NSScreen.screens.contains { $0.frame.contains(mouseLocation) }
    }
}
