// Deliver a `GURL` Apple Event to one specific process.
//
// This is the exact event LaunchServices sends when the user opens a
// `wisprlightning://` link — `kInternetEventClass` / `kAEGetURL` with the URL
// string as the direct object — which is what `tauri-plugin-deep-link`
// installs a handler for on macOS.
//
// It is addressed BY PID rather than by bundle identifier on purpose. The user
// of this machine also runs the commercial Wispr Flow-derived app from
// /Applications, which registers the SAME bundle identifier
// (`com.wisprlightning.app`) and the SAME two URL schemes. `open <url>` is
// therefore ambiguous and may hand the callback to the wrong process — see
// MATRIX AUT-004. Addressing the pid removes the ambiguity so the test measures
// our handler and nothing else.
//
// Usage: wlopenurl <pid> <url>

import CoreServices
import Foundation

// 'GURL' — `kInternetEventClass` and `kAEGetURL` are the same four-char code.
let gurl: UInt32 = 0x4755_524C

guard CommandLine.arguments.count == 3, let pid = Int32(CommandLine.arguments[1]) else {
    FileHandle.standardError.write("usage: wlopenurl <pid> <url>\n".data(using: .utf8)!)
    exit(2)
}
let url = CommandLine.arguments[2]

let target = NSAppleEventDescriptor(processIdentifier: pid)
let event = NSAppleEventDescriptor(
    eventClass: AEEventClass(gurl),
    eventID: AEEventID(gurl),
    targetDescriptor: target,
    returnID: AEReturnID(kAutoGenerateReturnID),
    transactionID: AETransactionID(kAnyTransactionID))
event.setParam(NSAppleEventDescriptor(string: url), forKeyword: AEKeyword(keyDirectObject))

do {
    // `.waitForReply` would block on an app that never answers a GURL event,
    // and the deep-link plugin does not answer. Fire and forget, then let the
    // caller assert on the app's own observable reaction.
    _ = try event.sendEvent(options: NSAppleEventDescriptor.SendOptions.noReply, timeout: 10)
    print("sent")
} catch {
    FileHandle.standardError.write("send failed: \(error)\n".data(using: .utf8)!)
    exit(1)
}
