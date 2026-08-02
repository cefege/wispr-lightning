import CryptoKit
import Foundation

// MARK: - Reference globals the app defines in AppDelegate.swift

// `wLog`/`wLogVerbose`/`isVerboseLoggingEnabled` are free functions declared at the
// top of `Sources/WisprLightning/App/AppDelegate.swift`, a file that imports AppKit
// and drags in the entire UI. The reference files this tool links call them, so the
// tool supplies its own no-op bindings. They only ever wrote to a log file, so
// stubbing them cannot change any value that ends up in a fixture.
var isVerboseLoggingEnabled: Bool = false
func wLog(_ message: String) {}
func wLogVerbose(_ message: String) {}

// MARK: - Sandboxed home

/// The reference `DatabaseManager`, `AppSettings` and `Session` all resolve their
/// on-disk locations through `FileManager.homeDirectoryForCurrentUser` at first touch
/// and expose no injection seam. Redirecting CoreFoundation's notion of the home
/// directory before anything reads it lets the generator drive the *real* storage
/// code against a throwaway tree instead of the developer's live database.
enum SandboxHome {
    /// Must be called before any code path can touch the home directory — in
    /// practice, as the first statement of `main.swift`.
    static func activate() -> URL {
        let base = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
            .appendingPathComponent("wispr-fixturegen-\(getpid())", isDirectory: true)
        try? FileManager.default.removeItem(at: base)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        setenv("CFFIXED_USER_HOME", base.path, 1)
        return base
    }
}

// MARK: - Errors

struct FixtureError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

// MARK: - Canonical JSON

/// Every JSON fixture is written with sorted keys so two runs — and the Rust port —
/// produce the same bytes. `JSONSerialization` orders a bridged Swift dictionary by
/// hash, and Swift seeds its hasher per process, so the wire order of the real frames
/// is genuinely unspecified; sorting is the only stable canonical form. Slashes are
/// left unescaped to match `serde_json`.
enum CanonicalJSON {
    static func encode(_ object: Any) throws -> Data {
        guard JSONSerialization.isValidJSONObject(object) else {
            throw FixtureError("value is not representable as JSON: \(object)")
        }
        var data = try JSONSerialization.data(
            withJSONObject: object,
            options: [.sortedKeys, .prettyPrinted, .withoutEscapingSlashes]
        )
        data.append(0x0A)
        return data
    }
}

// MARK: - Output tree

/// Writes fixtures beneath a root and accumulates a digest manifest as it goes.
final class FixtureTree {
    struct Entry {
        let path: String
        let bytes: Int
        let sha256: String
    }

    let root: URL
    private(set) var entries: [Entry] = []

    /// Subtrees and root-level files the generator owns outright. Everything here is
    /// deleted before a run so a file left behind by an older layout cannot survive and
    /// be committed as if it were still generated. Anything else in the root — notably
    /// the hand-written `README.md` — is left alone.
    private static let generatedPaths = [
        "append", "ascii85", "auth", "db", "pcm", "polish", "settings",
        "MANIFEST.json", "provenance.json"
    ]

    init(root: URL) throws {
        self.root = root
        for path in Self.generatedPaths {
            try? FileManager.default.removeItem(at: root.appendingPathComponent(path))
        }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }

    func write(_ data: Data, to relativePath: String) throws {
        let url = root.appendingPathComponent(relativePath)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try data.write(to: url)
        entries.append(Entry(path: relativePath, bytes: data.count, sha256: Digest.hex(data)))
    }

    func write(_ text: String, to relativePath: String) throws {
        try write(Data(text.utf8), to: relativePath)
    }

    func writeJSON(_ object: Any, to relativePath: String) throws {
        try write(CanonicalJSON.encode(object), to: relativePath)
    }

    /// Records a file produced outside this class (the SQLite database, which sqlite3
    /// writes itself) so it still appears in the manifest.
    func adopt(_ relativePath: String) throws {
        let url = root.appendingPathComponent(relativePath)
        let data = try Data(contentsOf: url)
        entries.append(Entry(path: relativePath, bytes: data.count, sha256: Digest.hex(data)))
    }

    func url(for relativePath: String) -> URL {
        root.appendingPathComponent(relativePath)
    }

    var totalBytes: Int { entries.reduce(0) { $0 + $1.bytes } }
}

enum Digest {
    static func hex(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    static func hexOfFile(at url: URL) throws -> String {
        hex(try Data(contentsOf: url))
    }
}

// MARK: - Deterministic pseudo-random source

/// A 64-bit LCG with the multiplier/increment from Knuth's MMIX. Integer-only by
/// design: every fixture byte must be reproducible on any machine, so nothing in the
/// generator is allowed to depend on the platform's libm.
struct Lcg {
    private var state: UInt64

    init(seed: UInt64) { self.state = seed }

    mutating func nextU64() -> UInt64 {
        state = state &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
        // Return the high bits; the low bits of an LCG have short periods.
        return state >> 16
    }

    mutating func nextByte() -> UInt8 { UInt8(truncatingIfNeeded: nextU64()) }

    mutating func bytes(_ count: Int) -> Data {
        var out = Data(capacity: count)
        for _ in 0..<count { out.append(nextByte()) }
        return out
    }
}
