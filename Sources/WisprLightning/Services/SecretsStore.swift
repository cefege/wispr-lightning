import Foundation

/// File-backed storage for per-vendor secrets that don't need to live in the
/// macOS Keychain. Writes a JSON file at `~/Library/Application Support/
/// WisprLightning/secrets.json` with `0600` permissions.
///
/// Why not Keychain? Each code-signature identity gets its own ACL on a
/// Keychain item, and unsigned / self-signed builds change cdhash on every
/// rebuild — which means macOS prompts the user to enter their login
/// password every time a build that didn't create the item tries to read
/// it. Storing in Application Support is plaintext but only readable by
/// the current user, and matches how Lightning already stores its Supabase
/// session file (Session.swift). The user explicitly preferred this tradeoff
/// for the BYO OpenRouter key (no prompts) over Keychain.
///
/// The Claude Code OAuth token mirror also writes here so subsequent reads
/// after the first cross-app prompt are silent.
enum SecretsStore {
    enum Key: String {
        case openRouterAPIKey
        /// JSON-encoded ClaudeCodeOAuthToken. Written after the user clicks
        /// Check (which triggers the one-time cross-app Keychain prompt for
        /// the `claude` CLI's item); subsequent reads come from this file
        /// instead of re-prompting on every signed rebuild.
        case claudeCodeTokenMirror
    }

    private static let lock = NSLock()
    private static var cache: [String: String]?

    private static let fileURL: URL = {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/WisprLightning")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("secrets.json")
    }()

    static func read(_ key: Key) -> String? {
        loadIfNeeded()
        lock.lock()
        defer { lock.unlock() }
        let value = cache?[key.rawValue]
        return (value?.isEmpty ?? true) ? nil : value
    }

    @discardableResult
    static func write(_ key: Key, _ value: String?) -> Bool {
        loadIfNeeded()
        lock.lock()
        let previous = cache ?? [:]
        var dict = previous
        if let value, !value.isEmpty {
            dict[key.rawValue] = value
        } else {
            dict.removeValue(forKey: key.rawValue)
        }
        lock.unlock()

        // Persist first, then update the cache — if the disk write fails we
        // don't want the in-process cache to diverge from what the next
        // launch's `loadIfNeeded()` will read off disk.
        guard persist(dict) else {
            // Restore cache to what's actually on disk so a future read
            // doesn't return a value that was never saved.
            lock.lock()
            cache = previous
            lock.unlock()
            return false
        }
        lock.lock()
        cache = dict
        lock.unlock()
        return true
    }

    @discardableResult
    static func delete(_ key: Key) -> Bool {
        return write(key, nil)
    }

    /// True if the key exists in the file. No deserialization or value
    /// transfer — safe to call on view appear to display "saved ✓" without
    /// triggering any prompt.
    static func has(_ key: Key) -> Bool {
        loadIfNeeded()
        lock.lock()
        defer { lock.unlock() }
        if let value = cache?[key.rawValue], !value.isEmpty { return true }
        return false
    }

    private static func loadIfNeeded() {
        lock.lock()
        defer { lock.unlock() }
        guard cache == nil else { return }
        // I/O under the lock is fine — secrets.json is a few hundred bytes at
        // most and read once per launch. Two threads racing here would
        // otherwise each parse the file independently.
        let data = (try? Data(contentsOf: fileURL)) ?? Data()
        cache = (try? JSONSerialization.jsonObject(with: data) as? [String: String]) ?? [:]
    }

    private static func persist(_ dict: [String: String]) -> Bool {
        guard let data = try? JSONSerialization.data(withJSONObject: dict, options: .prettyPrinted) else {
            return false
        }
        // Create the file with 0600 from the start so the contents are never
        // briefly world-readable. `Data.write(options:.atomic)` would use the
        // user's default umask (usually 022 → 644) and a follow-up chmod is
        // racy and silently fails.
        let attrs: [FileAttributeKey: Any] = [.posixPermissions: NSNumber(value: 0o600)]
        if FileManager.default.fileExists(atPath: fileURL.path) {
            try? FileManager.default.removeItem(at: fileURL)
        }
        let ok = FileManager.default.createFile(atPath: fileURL.path, contents: data, attributes: attrs)
        if !ok {
            wLog("SecretsStore: failed to write secrets file at \(fileURL.path)")
        }
        return ok
    }
}
