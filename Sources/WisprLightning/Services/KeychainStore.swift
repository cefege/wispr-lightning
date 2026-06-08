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

    /// Retrieve a stored value, or `nil` if not present / unreadable.
    /// Falls back to the legacy `com.wispr.edge` service and migrates the
    /// value over on first read so subsequent calls hit the new service.
    static func read(_ key: Key) -> String? {
        if let v = rawRead(service: service, account: key.rawValue) { return v }
        if let legacy = rawRead(service: legacyService, account: key.rawValue) {
            _ = write(key, legacy)
            return legacy
        }
        return nil
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
        let baseQuery: [String: Any] = [
            kSecClass as String:       kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key.rawValue,
        ]
        SecItemDelete(baseQuery as CFDictionary)
        guard let value = value, !value.isEmpty else { return true }

        var attrs = baseQuery
        attrs[kSecValueData as String] = Data(value.utf8)
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        let status = SecItemAdd(attrs as CFDictionary, nil)
        return status == errSecSuccess
    }

    @discardableResult
    static func delete(_ key: Key) -> Bool {
        return write(key, nil)
    }
}
