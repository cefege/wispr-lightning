import Foundation

/// Which backend currently handles dictation. Persisted as a string in settings
/// so the on-disk format survives future additions.
enum DictationVendor: String, CaseIterable {
    case wisprFlow = "wispr_flow"
    case openRouter = "openrouter"
    case claudeVoice = "claude_voice"

    var displayName: String {
        switch self {
        case .wisprFlow:   return "Wispr Flow"
        case .openRouter:  return "OpenRouter"
        case .claudeVoice: return "Claude Voice"
        }
    }
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
