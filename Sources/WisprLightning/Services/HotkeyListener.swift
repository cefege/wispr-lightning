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
    private var eventTap: CFMachPort?
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

    func start() {
        rebuildHotkeySet()
        installMonitors()
        setupEventTap()
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
        setupEventTap()
    }

    private func installMonitors() {
        let flagsMonitor = NSEvent.addGlobalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            self?.handleFlagsChanged(event)
        }
        if let m = flagsMonitor { monitors.append(m) }

        let localFlags = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            self?.handleFlagsChanged(event)
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

        if let tap = eventTap {
            CGEvent.tapEnable(tap: tap, enable: false)
            eventTap = nil
        }
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

    private func handleFlagsChanged(_ event: NSEvent) {
        let keycode = event.keyCode

        // Polish hotkey: modifier key used as standalone trigger
        if _polishKeyCodes.contains(keycode) && !hotkeySet.contains(keycode) {
            if Self.isModifierDown(keycode: keycode, flags: event.modifierFlags) {
                triggerPolish()
            }
            return
        }

        guard hotkeySet.contains(keycode) else { return }

        let isPressed = Self.isModifierDown(keycode: keycode, flags: event.modifierFlags)

        if isPressed && !keyDown {
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
        // Polish hotkey: regular key (only fires when CGEventTap is unavailable)
        if event.type == .keyDown && _polishKeyCodes.contains(event.keyCode) && !isModifierKeycode(event.keyCode) {
            triggerPolish()
            return
        }

        guard hotkeySet.contains(event.keyCode) else { return }
        guard !isModifierKeycode(event.keyCode) else { return }

        if event.type == .keyDown && !keyDown {
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

    private func isModifierKeycode(_ keycode: UInt16) -> Bool {
        return [59, 62, 58, 61, 55, 54, 56, 60, 63].contains(keycode)
    }

    private func setupEventTap() {
        let mask: CGEventMask = (1 << CGEventType.flagsChanged.rawValue) |
                                (1 << CGEventType.keyDown.rawValue) |
                                (1 << CGEventType.keyUp.rawValue)

        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .listenOnly,
            eventsOfInterest: mask,
            callback: { proxy, eventType, event, refcon -> Unmanaged<CGEvent>? in
                guard let refcon = refcon else { return Unmanaged.passUnretained(event) }
                let listener = Unmanaged<HotkeyListener>.fromOpaque(refcon).takeUnretainedValue()
                listener.handleCGEvent(type: eventType, event: event)
                return Unmanaged.passUnretained(event)
            },
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            NSLog("Wispr Lightning: CGEventTap not available — Accessibility permission may be needed")
            return
        }

        self.eventTap = tap
        let source = CFMachPortCreateRunLoopSource(nil, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        NSLog("Wispr Lightning: CGEventTap active")
    }

    private func handleCGEvent(type: CGEventType, event: CGEvent) {
        if type == .flagsChanged {
            let keycode = UInt16(event.getIntegerValueField(.keyboardEventKeycode))
            let flags = event.flags

            let isPressed: Bool
            switch keycode {
            case 59, 62: isPressed = flags.contains(.maskControl)
            case 58, 61: isPressed = flags.contains(.maskAlternate)
            case 55, 54: isPressed = flags.contains(.maskCommand)
            case 56, 60: isPressed = flags.contains(.maskShift)
            case 63: isPressed = flags.contains(.maskSecondaryFn)
            default: return
            }

            // Polish hotkey: modifier key used as standalone trigger
            if _polishKeyCodes.contains(keycode) && !hotkeySet.contains(keycode) {
                if isPressed { triggerPolish() }
                return
            }

            guard hotkeySet.contains(keycode) else { return }

            if isPressed && !keyDown {
                keyDown = true
                activeKeyCode = keycode
                DispatchQueue.main.async { self.onPress() }
            } else if !isPressed && keyDown && activeKeyCode == keycode {
                keyDown = false
                activeKeyCode = nil
                DispatchQueue.main.async { self.onRelease() }
            }
        } else if type == .keyDown {
            let keycode = UInt16(event.getIntegerValueField(.keyboardEventKeycode))

            // Polish hotkey: regular key
            if _polishKeyCodes.contains(keycode) && !isModifierKeycode(keycode) {
                triggerPolish()
                return
            }

            // Dictation hotkeys (non-modifier keys)
            guard hotkeySet.contains(keycode) else { return }
            guard !isModifierKeycode(keycode) else { return }

            if !keyDown {
                keyDown = true
                activeKeyCode = keycode
                DispatchQueue.main.async { self.onPress() }
            }
        } else if type == .keyUp {
            let keycode = UInt16(event.getIntegerValueField(.keyboardEventKeycode))
            if keyDown && activeKeyCode == keycode {
                keyDown = false
                activeKeyCode = nil
                DispatchQueue.main.async { self.onRelease() }
            }
        }
    }
}
