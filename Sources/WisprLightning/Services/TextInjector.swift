import AppKit
import CoreGraphics

class TextInjector {
    private let injectionQueue = DispatchQueue(label: "com.wisprlightning.textinjection")

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
            // Brief delay to ensure hotkey release is fully processed
            Thread.sleep(forTimeInterval: 0.01)
            self.pasteViaClipboard(text: text, completion: completion)
        }
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
