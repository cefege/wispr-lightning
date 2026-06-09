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
    /// Item written by the `claude` CLI. Read once, then mirrored into
    /// SecretsStore (file) so future reads don't trigger the cross-app
    /// password prompt.
    static let upstreamService = "Claude Code-credentials"
    /// Legacy Keychain mirror service — kept only for migration. Each
    /// signed rebuild had a different cdhash and the Keychain ACL re-prompt
    /// followed every install. The mirror now lives in SecretsStore which
    /// has none of that fragility.
    private static let legacyMirrorService = "com.wisprlightning"
    private static let legacyMirrorAccount = "claude_code.cached_token"

    /// In-process cache. Survives within a single launch — prevents repeated
    /// reads within one session.
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

        // Try the SecretsStore mirror first — file-backed, silent, no prompt.
        if !forceRefresh, let mirrored = readMirror(), !mirrored.isExpired {
            cacheLock.lock(); cachedToken = mirrored; cacheLock.unlock()
            return mirrored
        }

        // Either no mirror, mirror expired, or caller forced a refresh.
        // Read from the upstream `Claude Code-credentials` item — the
        // cross-app prompt happens here, at most once per token lifetime.
        let token = try readUpstream()
        writeMirror(token)
        // One-time cleanup: drop the legacy Keychain mirror if it still
        // exists (anyone upgrading from a build that used the old path).
        deleteLegacyMirror()
        cacheLock.lock(); cachedToken = token; cacheLock.unlock()
        return token
    }

    /// Drop both the in-process cache and the on-disk mirror. Used by
    /// Settings "Re-check" so the next read forces a fresh upstream pull.
    /// Also clears the legacy Keychain mirror in case it lingers.
    static func clearAllCaches() {
        cacheLock.lock()
        cachedToken = nil
        cacheLock.unlock()
        deleteMirror()
        deleteLegacyMirror()
    }

    /// Best-effort probe for whether the `claude` CLI is installed on this
    /// Mac. Checks the canonical install locations from `claude install` /
    /// Homebrew / npm. Used to show "Get the Claude CLI" hints in Settings
    /// without requiring the user to discover the error path themselves.
    static var isCLIInstalled: Bool {
        let candidates = [
            "/usr/local/bin/claude",
            "/opt/homebrew/bin/claude",
            "\(NSHomeDirectory())/.local/bin/claude",
            "\(NSHomeDirectory())/.npm/bin/claude",
            "\(NSHomeDirectory())/.bun/bin/claude",
        ]
        for path in candidates where FileManager.default.isExecutableFile(atPath: path) {
            return true
        }
        // Also check if PATH locates it. Avoid running the binary; just
        // probe directories.
        if let pathEnv = ProcessInfo.processInfo.environment["PATH"] {
            for dir in pathEnv.split(separator: ":") {
                if FileManager.default.isExecutableFile(atPath: "\(dir)/claude") {
                    return true
                }
            }
        }
        return false
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

    // MARK: - Mirror (SecretsStore — silent, file-backed)

    private static func readMirror() -> ClaudeCodeOAuthToken? {
        guard let json = SecretsStore.read(.claudeCodeTokenMirror),
              let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(ClaudeCodeOAuthToken.self, from: data)
    }

    private static func writeMirror(_ token: ClaudeCodeOAuthToken) {
        guard let data = try? JSONEncoder().encode(token),
              let json = String(data: data, encoding: .utf8) else { return }
        _ = SecretsStore.write(.claudeCodeTokenMirror, json)
    }

    private static func deleteMirror() {
        _ = SecretsStore.delete(.claudeCodeTokenMirror)
    }

    private static func deleteLegacyMirror() {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: legacyMirrorService,
            kSecAttrAccount as String: legacyMirrorAccount,
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
