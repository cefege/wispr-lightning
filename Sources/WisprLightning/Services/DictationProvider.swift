import Foundation

/// Which backend currently handles dictation. Persisted as a string in settings
/// so the on-disk format survives future additions.
enum DictationVendor: String, CaseIterable {
    case wisprFlow = "wispr_flow"
    case openRouter = "openrouter"
    case claudeVoice = "claude_voice"
    case deepgram = "deepgram"

    var displayName: String {
        switch self {
        case .wisprFlow:   return "Wispr Flow"
        case .openRouter:  return "OpenRouter"
        case .claudeVoice: return "Claude Voice"
        case .deepgram:    return "Deepgram"
        }
    }

    /// Lightweight, prompt-free check that this vendor has the credentials it
    /// needs to actually run a dictation. Used by the Provider chain UI to
    /// surface "Not signed in" badges before the first failed transcription.
    /// Conservative: returns true unless we can prove the vendor is unauth'd.
    func isReady(session: Session) -> Bool {
        switch self {
        case .wisprFlow:
            return session.isValid
        case .openRouter:
            return SecretsStore.has(.openRouterAPIKey)
                || KeychainStore.hasOpenRouterKeyHint()
                || ProcessInfo.processInfo.environment["WISPR_LIGHTNING_OPENROUTER_KEY"]?.isEmpty == false
        case .claudeVoice:
            // No prompt-free way to check the upstream Claude Code item, but
            // if the user has run `claude /login` the credentials file
            // exists at a known path. Best-effort indicator.
            let path = NSHomeDirectory() + "/.config/claude/credentials.json"
            return FileManager.default.fileExists(atPath: path)
                || ClaudeCodeCredentialFileLikelyExists()
        case .deepgram:
            return SecretsStore.has(.deepgramAPIKey)
                || ProcessInfo.processInfo.environment["WISPR_LIGHTNING_DEEPGRAM_KEY"]?.isEmpty == false
        }
    }
}

/// Some claude CLI versions keep credentials in the Keychain only, not on disk.
/// We can't probe the Keychain without prompting, so fall back to "unknown ready".
/// Returns true when we can't be confident the vendor is unready — i.e. when
/// absence of disk file isn't proof.
private func ClaudeCodeCredentialFileLikelyExists() -> Bool {
    // Conservative: we don't have a prompt-free signal, so don't claim "not
    // ready" — show the Claude Voice row without a warning. The user finds
    // out via the Check button if they want to be sure.
    return true
}

/// Final context for a dictation. Some providers (Wispr Flow) send this in
/// the auth message at stop-time; streaming providers (Claude Voice) may use
/// the parts they care about (e.g. keyterms) at start-time and ignore the rest.
struct DictationContext {
    let appInfo: [String: String]
    let ocrContext: [String]
    let axContext: [String]

    static let empty = DictationContext(appInfo: [:], ocrContext: [], axContext: [])
}

/// Abstraction over "audio in → cleaned text out".
///
/// Lightning was originally hardcoded against Wispr Flow's WebSocket API. This
/// protocol lets us plug in additional backends (OpenRouter, Claude Voice)
/// without touching the orchestration in `AppDelegate`.
///
/// Lifecycle:
///   prewarmConnection()        // optional, ahead of recording
///   start()                    // begin a new session
///   feed(packet:) * N          // PCM packets as they're captured
///   stop(context:, completion:) // finalize, deliver result
///   cancel()                   // abort without delivering
///
/// `feed(packet:)` may stream live (Claude Voice) or buffer internally
/// (Wispr Flow / OpenRouter) — that's a provider-level decision.
protocol DictationProvider: AnyObject {
    var dictionaryStore: DictionaryStore? { get set }

    func prewarmConnection()
    func cancelPrewarmedConnection()
    func clearEncodingCache()

    func start()
    func feed(packet: Data)
    func stop(context: DictationContext,
              completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void)
    func cancel()
}

extension DictationProvider {
    func prewarmConnection() {}
    func cancelPrewarmedConnection() {}
    func clearEncodingCache() {}
}
