// Dump every CoreGraphics window owned by a pid, including windows that are
// not on screen.
//
// The accessibility API (System Events) only ever reports windows that are
// visible, so it cannot distinguish "the overlay was never created" from "the
// overlay exists and is hidden" — which is exactly what MATRIX LIF-006 claims.
// `CGWindowListCopyWindowInfo(.optionAll, kCGNullWindowID)` does report
// off-screen windows, so that is what this reads.
//
// Window *titles* need Screen Recording, which this helper does not have, so
// nothing here depends on `kCGWindowName`. Owner pid, layer, bounds and the
// on-screen flag are all available without any TCC grant.
//
// Usage: wlwindows <pid>
// Output: one TSV line per window — id, onscreen, layer, x, y, w, h, alpha

import CoreGraphics
import Foundation

guard CommandLine.arguments.count == 2, let pid = Int(CommandLine.arguments[1]) else {
    FileHandle.standardError.write("usage: wlwindows <pid>\n".data(using: .utf8)!)
    exit(2)
}

guard
    let raw = CGWindowListCopyWindowInfo(
        [.optionAll, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]
else {
    FileHandle.standardError.write("CGWindowListCopyWindowInfo returned nothing\n".data(using: .utf8)!)
    exit(1)
}

for window in raw {
    guard let owner = window[kCGWindowOwnerPID as String] as? Int, owner == pid else { continue }
    let id = (window[kCGWindowNumber as String] as? Int) ?? -1
    let onscreen = (window[kCGWindowIsOnscreen as String] as? Bool) ?? false
    let layer = (window[kCGWindowLayer as String] as? Int) ?? 0
    let alpha = (window[kCGWindowAlpha as String] as? Double) ?? -1
    let bounds = window[kCGWindowBounds as String] as? [String: Double] ?? [:]
    let line = String(
        format: "id=%d\tonscreen=%@\tlayer=%d\tx=%.0f\ty=%.0f\tw=%.0f\th=%.0f\talpha=%.2f",
        id, onscreen ? "true" : "false", layer,
        bounds["X"] ?? -1, bounds["Y"] ?? -1, bounds["Width"] ?? -1, bounds["Height"] ?? -1,
        alpha)
    print(line)
}
