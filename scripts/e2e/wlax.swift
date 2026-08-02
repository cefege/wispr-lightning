// Read and drive another application's accessibility tree.
//
// System Events can do all of this, but a deep `entire contents` traversal of
// a WKWebView costs roughly two minutes per call and intermittently returns an
// empty list, which is not a basis for an assertion. This talks to the same
// AXUIElement API System Events talks to, from a process that Terminal is the
// responsible parent of, so it inherits the same Accessibility grant — it
// prints `trusted=false` and exits non-zero if it does not.
//
// Subcommands:
//   trusted                       — report whether AX access is available
//   dump   <pid>                  — the whole tree, one indented line per node
//   text   <pid>                  — every non-empty string in the tree
//   find   <pid> <substring>      — matching nodes with role and bounds
//   press  <pid> <substring>      — AXPress the first matching node
//   pick   <pid> <substring>      — AXPress, then list the resulting menu items
//   sheets <pid>                  — roles of the window's sheets, if any
//
// Matching is case-insensitive and compares against title, value, description
// and the accessibility label, because a Svelte button's text can land in any
// of them depending on how WebKit maps the element.

import ApplicationServices
import Foundation

let args = CommandLine.arguments
func die(_ message: String, _ code: Int32 = 2) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(code)
}

func attr(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else {
        return nil
    }
    return value
}

func str(_ element: AXUIElement, _ name: String) -> String? {
    guard let raw = attr(element, name) else { return nil }
    if let s = raw as? String { return s.isEmpty ? nil : s }
    if let n = raw as? NSNumber { return n.stringValue }
    return nil
}

func children(_ element: AXUIElement) -> [AXUIElement] {
    (attr(element, kAXChildrenAttribute as String) as? [AXUIElement]) ?? []
}

func role(_ element: AXUIElement) -> String {
    str(element, kAXRoleAttribute as String) ?? "?"
}

/// Every string a node carries, in the order WebKit is most likely to use.
func labels(_ element: AXUIElement) -> [String] {
    [kAXTitleAttribute, kAXValueAttribute, kAXDescriptionAttribute]
        .compactMap { str(element, $0 as String) }
        + [str(element, "AXLabel")].compactMap { $0 }
}

func describe(_ element: AXUIElement) -> String {
    let text = labels(element).joined(separator: " | ")
    return text.isEmpty ? role(element) : "\(role(element)) [\(text)]"
}

func walk(_ element: AXUIElement, _ depth: Int, _ visit: (AXUIElement, Int) -> Void) {
    visit(element, depth)
    if depth > 40 { return }
    for child in children(element) { walk(child, depth + 1, visit) }
}

func matches(_ element: AXUIElement, _ needle: String) -> Bool {
    let lowered = needle.lowercased()
    return labels(element).contains { $0.lowercased().contains(lowered) }
}

func root(_ pid: pid_t) -> AXUIElement { AXUIElementCreateApplication(pid) }

func bounds(_ element: AXUIElement) -> String {
    var origin = CGPoint.zero
    var size = CGSize.zero
    if let p = attr(element, kAXPositionAttribute as String) {
        AXValueGetValue(p as! AXValue, .cgPoint, &origin)
    }
    if let s = attr(element, kAXSizeAttribute as String) {
        AXValueGetValue(s as! AXValue, .cgSize, &size)
    }
    return String(format: "%.0f,%.0f %.0fx%.0f", origin.x, origin.y, size.width, size.height)
}

guard args.count >= 2 else { die("usage: wlax <trusted|dump|text|find|press|pick|sheets> ...") }
let command = args[1]

if command == "trusted" {
    print("trusted=\(AXIsProcessTrusted())")
    exit(AXIsProcessTrusted() ? 0 : 1)
}

guard AXIsProcessTrusted() else { die("trusted=false — no Accessibility grant", 3) }
guard args.count >= 3, let pid = pid_t(args[2]) else { die("usage: wlax \(command) <pid> ...") }
let app = root(pid)

/// Every window of the app. The app's own menu bars dwarf the window content —
/// roughly 6000 nodes of Apple menu, Recent Items and Services against a few
/// hundred of actual UI — so anything reading the interface starts here.
let windows = (attr(app, kAXWindowsAttribute as String) as? [AXUIElement]) ?? []

switch command {
case "dump":
    walk(app, 0) { element, depth in
        print(String(repeating: "  ", count: depth) + describe(element))
    }

case "text":
    var seen = Set<String>()
    walk(app, 0) { element, _ in
        for label in labels(element) where !seen.contains(label) {
            seen.insert(label)
            print(label)
        }
    }

case "pane":
    // `text` restricted to the window tree, so a caller can assert on what the
    // settings UI says without the menu bar drowning it.
    var shown = Set<String>()
    for window in windows {
        walk(window, 0) { element, _ in
            for label in labels(element) where !shown.contains(label) {
                shown.insert(label)
                print(label)
            }
        }
    }
    exit(windows.isEmpty ? 1 : 0)

case "find":
    guard args.count >= 4 else { die("usage: wlax find <pid> <substring>") }
    var hits = 0
    walk(app, 0) { element, _ in
        if matches(element, args[3]) {
            hits += 1
            print("\(describe(element))  @ \(bounds(element))")
        }
    }
    exit(hits > 0 ? 0 : 1)

case "press", "pick":
    guard args.count >= 4 else { die("usage: wlax \(command) <pid> <substring>") }
    var target: AXUIElement?
    walk(app, 0) { element, _ in
        // Prefer an actionable node: a WebKit button's text often also appears
        // on the static text inside it, and pressing the text does nothing.
        guard target == nil, matches(element, args[3]) else { return }
        let r = role(element)
        if r == kAXButtonRole as String || r == kAXPopUpButtonRole as String
            || r == kAXRadioButtonRole as String || r == kAXCheckBoxRole as String
            || r == kAXMenuItemRole as String || r == "AXLink"
        {
            target = element
        }
    }
    if target == nil {
        // Fall back to the nearest actionable ancestor of any textual match.
        walk(app, 0) { element, _ in
            guard target == nil, matches(element, args[3]) else { return }
            var node: AXUIElement? = element
            for _ in 0..<4 {
                guard let current = node else { break }
                var names: CFArray?
                if AXUIElementCopyActionNames(current, &names) == .success,
                    let list = names as? [String], list.contains(kAXPressAction as String)
                {
                    target = current
                    return
                }
                node = attr(current, kAXParentAttribute as String).map { $0 as! AXUIElement }
            }
        }
    }
    guard let element = target else { die("no pressable element matching \(args[3])", 1) }
    let result = AXUIElementPerformAction(element, kAXPressAction as CFString)
    guard result == .success else { die("AXPress failed: \(result.rawValue)", 1) }
    print("pressed \(describe(element))")
    if command == "pick" {
        // A native <select> opens an AXMenu; give WebKit a moment to build it.
        Thread.sleep(forTimeInterval: 1.0)
        walk(root(pid), 0) { node, _ in
            if role(node) == kAXMenuItemRole as String {
                print("item: " + (labels(node).first ?? ""))
            }
        }
    }

case "sheets":
    // A native open/save panel is an AXSheet on the window that raised it, so
    // this is how "did a file dialog appear?" is answered without guessing.
    print("windows=\(windows.count)")
    for window in windows {
        let sheets = (attr(window, "AXSheets") as? [AXUIElement]) ?? []
        print("window \(str(window, kAXTitleAttribute as String) ?? "?"): sheets=\(sheets.count)")
        for sheet in sheets { print("  sheet " + describe(sheet)) }
    }

default:
    die("unknown command \(command)")
}
