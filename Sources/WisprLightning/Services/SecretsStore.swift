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
        var dict = cache ?? [:]
        if let value, !value.isEmpty {
            dict[key.rawValue] = value
        } else {
            dict.removeValue(forKey: key.rawValue)
        }
        cache = dict
        lock.unlock()
        return persist(dict)
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
        let alreadyLoaded = cache != nil
        lock.unlock()
        if alreadyLoaded { return }

        let data = (try? Data(contentsOf: fileURL)) ?? Data()
        let dict = (try? JSONSerialization.jsonObject(with: data) as? [String: String]) ?? [:]
        lock.lock()
        cache = dict
        lock.unlock()
    }

    private static func persist(_ dict: [String: String]) -> Bool {
        guard let data = try? JSONSerialization.data(withJSONObject: dict, options: .prettyPrinted) else {
            return false
        }
        do {
            try data.write(to: fileURL, options: .atomic)
            // Restrict to owner only — same posture as session.json.
            try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: fileURL.path)
            return true
        } catch {
            return false
        }
    }
}
