import AppKit
import Carbon.HIToolbox
import CoreGraphics

class TextInjector {
    private let injectionQueue = DispatchQueue(label: "com.wisprlightning.textinjection")
    private let settings: AppSettings

    /// Reverse map of the current keyboard layout. Cached and rebuilt only
    /// when the active input source changes mid-session.
    private var layoutMap: [Character: (keyCode: UInt16, flags: CGEventFlags)] = [:]
    private var layoutMapSourceID: String?

    /// Flipped to `true` from the main-thread Esc monitor; read between
    /// characters by the typing loop on `injectionQueue`.
    private let cancelLock = NSLock()
    private var _cancelTyping = false

    init(settings: AppSettings) {
        self.settings = settings
    }

    private func setCancelTyping(_ value: Bool) {
        cancelLock.lock()
        defer { cancelLock.unlock() }
        _cancelTyping = value
    }

    private func isCancelTyping() -> Bool {
        cancelLock.lock()
        defer { cancelLock.unlock() }
        return _cancelTyping
    }

    /// Read the currently selected text via Accessibility API.
    /// Returns the selected text string, or nil if no selection.
    static func readSelectedText() -> String? {
        let systemWide = AXUIElementCreateSystemWide()
        var focusedElement: AnyObject?
        let focusResult = AXUIElementCopyAttributeValue(systemWide, kAXFocusedUIElementAttribute as CFString, &focusedElement)
        guard focusResult == .success, let focused = focusedElement else {
            return nil
        }
        let element = unsafeBitCast(focused, to: AXUIElement.self)
        var value: AnyObject?
        let valueResult = AXUIElementCopyAttributeValue(element, kAXSelectedTextAttribute as CFString, &value)
        guard valueResult == .success, let text = value as? String, !text.isEmpty else {
            return nil
        }
        return text
    }

    /// Snapshot the current pasteboard. Must be called from a non-main thread.
    static func saveClipboard() -> [[(NSPasteboard.PasteboardType, Data)]] {
        var saved: [[(NSPasteboard.PasteboardType, Data)]] = []
        DispatchQueue.main.sync {
            for item in NSPasteboard.general.pasteboardItems ?? [] {
                var pairs: [(NSPasteboard.PasteboardType, Data)] = []
                for type in item.types {
                    if let data = item.data(forType: type) { pairs.append((type, data)) }
                }
                if !pairs.isEmpty { saved.append(pairs) }
            }
        }
        return saved
    }

    /// Restore a previously saved pasteboard snapshot. Must be called on the main thread.
    static func restoreClipboard(_ items: [[(NSPasteboard.PasteboardType, Data)]]) {
        guard !items.isEmpty else { return }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        for itemData in items {
            let newItem = NSPasteboardItem()
            for (type, data) in itemData { newItem.setData(data, forType: type) }
            pasteboard.writeObjects([newItem])
        }
    }

    /// Read the focused text field's current value via Accessibility API.
    /// Returns the text as a single-element array, or empty array if unavailable.
    static func readFocusedElementText() -> [String] {
        let systemWide = AXUIElementCreateSystemWide()
        var focusedElement: AnyObject?
        let focusResult = AXUIElementCopyAttributeValue(systemWide, kAXFocusedUIElementAttribute as CFString, &focusedElement)
        guard focusResult == .success, let focused = focusedElement else {
            return []
        }
        let element = unsafeBitCast(focused, to: AXUIElement.self)
        var value: AnyObject?
        let valueResult = AXUIElementCopyAttributeValue(element, kAXValueAttribute as CFString, &value)
        guard valueResult == .success, let text = value as? String, !text.isEmpty else {
            return []
        }
        return [text]
    }

    func inject(text: String, completion: @escaping (_ pasteSucceeded: Bool) -> Void) {
        guard !text.isEmpty else {
            completion(false)
            return
        }
        wLog("TextInjector.inject called with \(text.count) chars")

        injectionQueue.async {
            // Wait for the hotkey release to settle before posting events.
            Thread.sleep(forTimeInterval: 0.01)
            if self.settings.naturalModeEnabled {
                self.typeAsKeystrokes(text: text, completion: completion)
            } else {
                self.pasteViaClipboard(text: text, completion: completion)
            }
        }
    }

    /// Calibrated to slow ≈ 30 WPM, normal ≈ 50 WPM, expert ≈ 80 WPM
    /// (5 chars per word).
    private func charsPerSecond(for preset: String) -> Double {
        switch preset {
        case "slow":   return 2.5
        case "expert": return 6.5
        default:       return 4.0
        }
    }

    private func typeAsKeystrokes(text: String, completion: @escaping (_ pasteSucceeded: Bool) -> Void) {
        let cps = charsPerSecond(for: settings.naturalModeSpeed)
        let baseDelay = 1.0 / cps

        // Private state isolates synthesized events from the user's hardware
        // keyboard. Without this, ambient modifier state (Caps Lock, residual
        // Shift from the hotkey release) bleeds in and corrupts output:
        // `,` → `<`, `'` → `"`, lowercase flips to uppercase under Caps Lock.
        guard let source = CGEventSource(stateID: .privateState) else {
            wLog("Natural Mode: failed to create CGEventSource — falling back to paste")
            pasteViaClipboard(text: text, completion: completion)
            return
        }

        setCancelTyping(false)
        var globalMonitor: Any?
        var localMonitor: Any?
        // Local monitor swallows Esc inside our app; the global monitor can
        // only observe — Esc still reaches the focused target app.
        DispatchQueue.main.sync {
            globalMonitor = NSEvent.addGlobalMonitorForEvents(matching: .keyDown) { [weak self] event in
                if Int(event.keyCode) == kVK_Escape { self?.setCancelTyping(true) }
            }
            localMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                if Int(event.keyCode) == kVK_Escape {
                    self?.setCancelTyping(true)
                    return nil
                }
                return event
            }
            // TIS asserts main-thread; build/refresh while we're already here.
            self.ensureLayoutMap()
        }
        wLog("Natural Mode typing \(text.count) chars at \(cps) cps (layout map: \(layoutMap.count) entries)")

        var typed = 0
        for ch in text {
            if isCancelTyping() {
                wLog("Natural Mode: cancelled by Esc after \(typed)/\(text.count) chars")
                break
            }
            postCharacter(ch, source: source)
            typed += 1
            // Jitter so timing doesn't look mechanical.
            let jitter = Double.random(in: 0.6...1.4)
            Thread.sleep(forTimeInterval: baseDelay * jitter)
        }

        DispatchQueue.main.async {
            if let g = globalMonitor { NSEvent.removeMonitor(g) }
            if let l = localMonitor { NSEvent.removeMonitor(l) }
        }

        completion(true)
    }

    /// Falls back to unicode string injection only for characters with no key
    /// on the current layout (e.g. emoji, CJK on a Latin layout).
    private func postCharacter(_ ch: Character, source: CGEventSource) {
        // Bare Return submits in most chat apps (Slack, Discord, ChatGPT,
        // Claude Code's prompt) and executes in shells, so dictating a
        // newline would send the message early. Shift+Return is the
        // "newline without submit" convention. Raw shells still submit on
        // either form — that's a known limitation.
        if ch == "\n" {
            postKey(virtualKey: UInt16(kVK_Return), flags: [.maskShift], source: source)
            return
        }
        if ch == "\t" {
            postKey(virtualKey: UInt16(kVK_Tab), flags: [], source: source)
            return
        }
        if let mapped = layoutMap[ch] {
            postKey(virtualKey: mapped.keyCode, flags: mapped.flags, source: source)
        } else {
            postUnicodeFallback(ch, source: source)
        }
    }

    private func postKey(virtualKey: UInt16, flags: CGEventFlags, source: CGEventSource) {
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: false) else { return }
        // Pin flags unconditionally; otherwise ambient modifier state rides
        // along and corrupts punctuation (`,` → `<`, `'` → `"`).
        down.flags = flags
        up.flags = flags
        down.post(tap: .cghidEventTap)
        // 30-80ms hold so events register as a press, not a glitch.
        Thread.sleep(forTimeInterval: Double.random(in: 0.030...0.080))
        up.post(tap: .cghidEventTap)
    }

    private func postUnicodeFallback(_ ch: Character, source: CGEventSource) {
        let utf16 = Array(String(ch).utf16)
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false) else { return }
        down.flags = []
        up.flags = []
        utf16.withUnsafeBufferPointer { buf in
            if let base = buf.baseAddress {
                down.keyboardSetUnicodeString(stringLength: buf.count, unicodeString: base)
                up.keyboardSetUnicodeString(stringLength: buf.count, unicodeString: base)
            }
        }
        down.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: Double.random(in: 0.030...0.080))
        up.post(tap: .cghidEventTap)
    }

    /// Reverse-maps every character reachable on the current keyboard layout
    /// to its (virtualKey, flags) by iterating virtual keys × modifier combos
    /// through `UCKeyTranslate`.
    private func ensureLayoutMap() {
        guard let source = TISCopyCurrentKeyboardLayoutInputSource()?.takeRetainedValue() else { return }
        let idPtr = TISGetInputSourceProperty(source, kTISPropertyInputSourceID)
        let currentID = idPtr.map { Unmanaged<CFString>.fromOpaque($0).takeUnretainedValue() as String } ?? ""
        if currentID == layoutMapSourceID, !layoutMap.isEmpty { return }

        guard let layoutDataPtr = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else {
            wLog("Natural Mode: keyboard layout data unavailable")
            return
        }
        let layoutData = Unmanaged<CFData>.fromOpaque(layoutDataPtr).takeUnretainedValue()
        guard let bytes = CFDataGetBytePtr(layoutData) else {
            wLog("Natural Mode: empty keyboard layout data")
            return
        }
        let keyboardLayout = UnsafeRawPointer(bytes).assumingMemoryBound(to: UCKeyboardLayout.self)

        // Cmd is omitted: it suppresses character generation in UCKeyTranslate.
        let modCombos: [(UInt32, CGEventFlags)] = [
            (0,                                                    []),
            (UInt32(shiftKey >> 8),                                .maskShift),
            (UInt32(optionKey >> 8),                               .maskAlternate),
            (UInt32((shiftKey | optionKey) >> 8),                  [.maskShift, .maskAlternate]),
        ]

        var newMap: [Character: (UInt16, CGEventFlags)] = [:]
        let kbdType = UInt32(LMGetKbdType())

        for keyCode in UInt16(0)..<UInt16(128) {
            for (modKey, flags) in modCombos {
                var deadKeyState: UInt32 = 0
                var chars = [UniChar](repeating: 0, count: 4)
                var actualLen = 0
                let err = UCKeyTranslate(
                    keyboardLayout,
                    keyCode,
                    UInt16(kUCKeyActionDown),
                    modKey,
                    kbdType,
                    UInt32(kUCKeyTranslateNoDeadKeysBit),
                    &deadKeyState,
                    chars.count,
                    &actualLen,
                    &chars
                )
                guard err == noErr, actualLen > 0 else { continue }
                let s = String(utf16CodeUnits: chars, count: actualLen)
                guard s.count == 1, let ch = s.first else { continue }
                // Skip control chars; we handle \n and \t explicitly.
                if let scalar = ch.unicodeScalars.first, scalar.value < 0x20 { continue }
                if newMap[ch] == nil {
                    newMap[ch] = (keyCode, flags)
                }
            }
        }

        layoutMap = newMap
        layoutMapSourceID = currentID
    }

    private func pasteViaClipboard(text: String, completion: @escaping (_ pasteSucceeded: Bool) -> Void) {
        let savedItems = Self.saveClipboard()
        DispatchQueue.main.sync {
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            pasteboard.setString(text, forType: .string)
        }

        wLog("Clipboard set, simulating Cmd+V")

        // Simulate Cmd+V from background thread (CGEvent is thread-safe)
        let source = CGEventSource(stateID: .hidSystemState)
        guard let keyDown = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: true),
              let keyUp = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: false) else {
            wLog("Failed to create Cmd+V CGEvent — check Accessibility permissions")
            completion(false)
            return
        }

        keyDown.flags = .maskCommand
        keyUp.flags = .maskCommand
        keyDown.post(tap: .cghidEventTap)
        keyUp.post(tap: .cghidEventTap)

        wLog("Cmd+V posted")

        // Wait for paste to be processed, then verify
        Thread.sleep(forTimeInterval: 0.05)

        let pasteOK = verifyPaste(expected: text)

        // Restore old clipboard after paste is consumed
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
            Self.restoreClipboard(savedItems)
            if !savedItems.isEmpty {
                NSLog("Wispr Lightning: Clipboard restored (%d items)", savedItems.count)
            }
        }

        if pasteOK {
            completion(true)
        } else {
            wLog("Paste verification failed — clipboard still restored")
            completion(false)
        }
    }

    private func verifyPaste(expected: String) -> Bool {
        let systemWide = AXUIElementCreateSystemWide()
        var focusedElement: AnyObject?
        let focusResult = AXUIElementCopyAttributeValue(systemWide, kAXFocusedUIElementAttribute as CFString, &focusedElement)
        guard focusResult == .success, let focused = focusedElement else {
            wLog("Paste verify: no focused element — assuming success")
            return true
        }
        let element = unsafeBitCast(focused, to: AXUIElement.self)
        var value: AnyObject?
        let valueResult = AXUIElementCopyAttributeValue(element, kAXValueAttribute as CFString, &value)
        guard valueResult == .success, let text = value as? String else {
            wLog("Paste verify: could not read value attribute — assuming success")
            return true
        }
        let prefix = String(expected.prefix(20))
        return text.contains(prefix)
    }
}
