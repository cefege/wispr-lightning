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
    static let service = "Claude Code-credentials"

    /// Why: after macOS sleep / long idle, the login keychain auto-relocks and
    /// the next SecItemCopyMatching blocks for several seconds while it
    /// re-unlocks. We cache the token in process memory so we hit the keychain
    /// at most once per launch (or after a forceRefresh).
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

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)

        guard status != errSecItemNotFound else { throw ClaudeCodeKeychainError.itemNotFound }
        guard status == errSecSuccess, let data = item as? Data else {
            throw ClaudeCodeKeychainError.readFailed(status)
        }
        let token = try decode(data)
        cacheLock.lock()
        cachedToken = token
        cacheLock.unlock()
        return token
    }

    static func clearCache() {
        cacheLock.lock()
        cachedToken = nil
        cacheLock.unlock()
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
