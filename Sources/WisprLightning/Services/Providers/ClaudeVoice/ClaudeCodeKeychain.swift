import Foundation
import Security

/// Reads the `Claude Code-credentials` Keychain entry that the `claude` CLI
/// writes when the user runs `claude /login`. Lightning never writes to this
/// item — it's owned by the CLI; we just consume it. On expiry we surface
/// "Run `claude /login`" and let the user refresh.
struct ClaudeCodeOAuthToken: Codable {
    let accessToken: String
    let refreshToken: String?
    let expiresAt: Int64?
    let scopes: [String]?
    let subscriptionType: String?

    enum CodingKeys: String, CodingKey {
        case accessToken
        case refreshToken
        case expiresAt
        case scopes
        case subscriptionType
    }

    var isExpired: Bool {
        guard let expiresAt = expiresAt else { return false }
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        return nowMs >= expiresAt
    }
}

private struct ClaudeCodeCredentialsEnvelope: Codable {
    let claudeAiOauth: ClaudeCodeOAuthToken
}

enum ClaudeCodeKeychainError: Error, CustomStringConvertible {
    case itemNotFound
    case readFailed(OSStatus)
    case decodeFailed(String)

    var description: String {
        switch self {
        case .itemNotFound:
            return "No 'Claude Code-credentials' item in Keychain. Run `claude /login` first."
        case .readFailed(let status):
            return "Keychain read failed (OSStatus \(status))."
        case .decodeFailed(let msg):
            return "Could not decode credentials JSON: \(msg)"
        }
    }
}

enum ClaudeCodeKeychain {
    /// Item written by the `claude` CLI. Read once, then mirrored into our
    /// own Keychain service so future reads don't trigger the cross-app
    /// password prompt.
    static let upstreamService = "Claude Code-credentials"
    /// Our own mirror. Lightning owns this item, so SecItemCopyMatching
    /// against it never prompts. Re-synced from the upstream item whenever
    /// the mirror is missing or expired.
    private static let mirrorService = "com.wisprlightning"
    private static let mirrorAccount = "claude_code.cached_token"

    /// In-process cache. Survives within a single launch — prevents repeated
    /// keychain reads within one session.
    private static let cacheLock = NSLock()
    private static var cachedToken: ClaudeCodeOAuthToken?

    static func read(forceRefresh: Bool = false) throws -> ClaudeCodeOAuthToken {
        if !forceRefresh {
            cacheLock.lock()
            let cached = cachedToken
            cacheLock.unlock()
            if let cached, !cached.isExpired {
                return cached
            }
        }

        // Try our own mirrored copy first — silent, no cross-app prompt.
        if !forceRefresh, let mirrored = readMirror(), !mirrored.isExpired {
            cacheLock.lock(); cachedToken = mirrored; cacheLock.unlock()
            return mirrored
        }

        // Either no mirror, mirror expired, or caller forced a refresh.
        // Read from the upstream `Claude Code-credentials` item — the prompt
        // happens here, at most once per token lifetime.
        let token = try readUpstream()
        writeMirror(token)
        cacheLock.lock(); cachedToken = token; cacheLock.unlock()
        return token
    }

    /// Drop both the in-process cache and the on-disk mirror. Used by
    /// Settings "Re-check" so the next read forces a fresh upstream pull.
    static func clearAllCaches() {
        cacheLock.lock()
        cachedToken = nil
        cacheLock.unlock()
        deleteMirror()
    }

    // MARK: - Upstream (claude CLI's item)

    private static func readUpstream() throws -> ClaudeCodeOAuthToken {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: upstreamService,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)

        guard status != errSecItemNotFound else { throw ClaudeCodeKeychainError.itemNotFound }
        guard status == errSecSuccess, let data = item as? Data else {
            throw ClaudeCodeKeychainError.readFailed(status)
        }
        return try decode(data)
    }

    // MARK: - Mirror (our own item — silent reads)

    private static func readMirror() -> ClaudeCodeOAuthToken? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: mirrorService,
            kSecAttrAccount as String: mirrorAccount,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess, let data = item as? Data else { return nil }
        return try? JSONDecoder().decode(ClaudeCodeOAuthToken.self, from: data)
    }

    private static func writeMirror(_ token: ClaudeCodeOAuthToken) {
        guard let data = try? JSONEncoder().encode(token) else { return }
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: mirrorService,
            kSecAttrAccount as String: mirrorAccount,
        ]
        SecItemDelete(base as CFDictionary)
        var attrs = base
        attrs[kSecValueData as String] = data
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        _ = SecItemAdd(attrs as CFDictionary, nil)
    }

    private static func deleteMirror() {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: mirrorService,
            kSecAttrAccount as String: mirrorAccount,
        ]
        SecItemDelete(query as CFDictionary)
    }

    private static func decode(_ data: Data) throws -> ClaudeCodeOAuthToken {
        let decoder = JSONDecoder()
        do {
            let envelope = try decoder.decode(ClaudeCodeCredentialsEnvelope.self, from: data)
            return envelope.claudeAiOauth
        } catch {
            throw ClaudeCodeKeychainError.decodeFailed(String(describing: error))
        }
    }
}
