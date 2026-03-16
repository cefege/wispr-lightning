import AppKit
import CoreGraphics

class TextInjector {
    private let injectionQueue = DispatchQueue(label: "com.wisprlite.textinjection")

    func inject(text: String, completion: @escaping (_ pasteSucceeded: Bool) -> Void) {
        guard !text.isEmpty else {
            completion(false)
            return
        }
        wLog("TextInjector.inject called with \(text.count) chars")

        injectionQueue.async {
            // Delay to ensure hotkey release is fully processed
            Thread.sleep(forTimeInterval: 0.1)
            self.pasteViaClipboard(text: text, completion: completion)
        }
    }

    private func pasteViaClipboard(text: String, completion: @escaping (_ pasteSucceeded: Bool) -> Void) {
        // Pasteboard operations must happen on main thread
        var oldContents: String? = nil
        DispatchQueue.main.sync {
            let pasteboard = NSPasteboard.general
            oldContents = pasteboard.string(forType: .string)
            pasteboard.clearContents()
            pasteboard.setString(text, forType: .string)
        }

        wLog("Clipboard set, simulating Cmd+V")

        // Small delay to ensure pasteboard is ready
        Thread.sleep(forTimeInterval: 0.05)

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
        Thread.sleep(forTimeInterval: 0.3)

        let pasteOK = verifyPaste(expected: text)

        // Always restore old clipboard after a delay
        let saved = oldContents
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            if let old = saved {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(old, forType: .string)
                NSLog("Wispr Lite: Clipboard restored")
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
