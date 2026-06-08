import Foundation
import Security

/// Thin wrapper around the macOS Keychain for storing per-vendor API keys.
///
/// Stored as `kSecClassGenericPassword` with `service = "com.wisprlightning"`
/// plus a per-key account string. Strings are UTF-8 encoded in the keychain value.
enum KeychainStore {
    private static let service = "com.wisprlightning"
    /// Legacy service id used by the standalone `wispr-edge` app. Lightning
    /// reads from there as a fallback (and migrates the value on first hit)
    /// so users coming from Wispr Edge don't have to re-paste their key.
    private static let legacyService = "com.wispr.edge"

    enum Key: String {
        case openRouterAPIKey = "openrouter.api_key"
    }

    /// Process-wide in-memory cache. Without this, every Settings open
    /// (SettingsViewModel.init) and every dictation (OpenRouterProvider.apiKey)
    /// triggers SecItemCopyMatching — and if the item was written by a
    /// differently-signed prior build, every one of those reads prompts
    /// the user for their login password. Caching means one prompt per
    /// launch at worst.
    private static let cacheLock = NSLock()
    private static var cache: [Key: String] = [:]

    /// Retrieve a stored value, or `nil` if not present / unreadable.
    /// Falls back to the legacy `com.wispr.edge` service and migrates the
    /// value over on first read so subsequent calls hit the new service.
    /// Returns from in-process cache when available.
    static func read(_ key: Key) -> String? {
        cacheLock.lock()
        let cached = cache[key]
        cacheLock.unlock()
        if let cached { return cached }

        let resolved: String?
        let cameFromLegacy: Bool
        if let v = rawRead(service: service, account: key.rawValue) {
            resolved = v
            cameFromLegacy = false
        } else if let legacy = rawRead(service: legacyService, account: key.rawValue) {
            resolved = legacy
            cameFromLegacy = true
        } else {
            resolved = nil
            cameFromLegacy = false
        }

        if let resolved {
            // Rewrite the item back to our own service. If we read it from
            // the legacy service this is the migration step. If we read it
            // from our own service it's an ownership-rotation step: the
            // entry might have been written by a prior unsigned / differently
            // signed build whose code identity no longer matches, which is
            // exactly what triggers the "enter your login password" prompt
            // every launch. Rewriting under the current code identity makes
            // the next launch's read silent.
            _ = writeRaw(service: service, account: key.rawValue, value: resolved)
            if cameFromLegacy {
                // Also drop the legacy entry so we don't re-prompt on it later.
                _ = writeRaw(service: legacyService, account: key.rawValue, value: nil)
            }
            cacheLock.lock()
            cache[key] = resolved
            cacheLock.unlock()
        }
        return resolved
    }

    private static func rawRead(service: String, account: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String:       kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecMatchLimit as String:  kSecMatchLimitOne,
            kSecReturnData as String:  true,
        ]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess, let data = item as? Data, let string = String(data: data, encoding: .utf8) else {
            return nil
        }
        let trimmed = string.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    /// Store (or replace) a value. Passing `nil` deletes the entry.
    @discardableResult
    static func write(_ key: Key, _ value: String?) -> Bool {
        let ok = writeRaw(service: service, account: key.rawValue, value: value)
        cacheLock.lock()
        if let value, !value.isEmpty {
            cache[key] = value
        } else {
            cache.removeValue(forKey: key)
        }
        cacheLock.unlock()
        return ok
    }

    @discardableResult
    static func delete(_ key: Key) -> Bool {
        return write(key, nil)
    }

    @discardableResult
    private static func writeRaw(service: String, account: String, value: String?) -> Bool {
        let baseQuery: [String: Any] = [
            kSecClass as String:       kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(baseQuery as CFDictionary)
        guard let value = value, !value.isEmpty else { return true }

        var attrs = baseQuery
        attrs[kSecValueData as String] = Data(value.utf8)
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        let status = SecItemAdd(attrs as CFDictionary, nil)
        return status == errSecSuccess
    }
}
