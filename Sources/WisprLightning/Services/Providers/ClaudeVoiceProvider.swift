import Foundation

/// `DictationProvider` backed by Claude Code's STT WebSocket (the only
/// truly streaming backend). Reads auth from the `Claude Code-credentials`
/// Keychain entry that the `claude` CLI manages — Lightning never writes to it.
///
/// On `start()` we open the WebSocket and begin sending PCM live as packets
/// arrive via `feed(packet:)`. On `stop(context:)` we send CloseStream and
/// await the server's final TranscriptEndpoint. Finals from intermediate
/// utterances are concatenated to form the returned transcript.
final class ClaudeVoiceProvider: NSObject, DictationProvider, VoiceStreamDelegate {
    var dictionaryStore: DictionaryStore?

    private let settings: AppSettings

    private var stream: VoiceStream?
    private var finals: [String] = []
    private let finalsLock = NSLock()
    private var packetCount: Int = 0
    private var failureMessage: String?
    private let queue = DispatchQueue(label: "WisprLightning.ClaudeVoiceProvider")
    private var inSession = false

    init(settings: AppSettings) {
        self.settings = settings
    }

    // MARK: - DictationProvider lifecycle

    func start() {
        queue.async { [weak self] in
            self?.beginSession()
        }
    }

    func feed(packet: Data) {
        queue.async { [weak self] in
            guard let self else { return }
            self.packetCount += 1
            self.stream?.send(pcm: packet)
        }
    }

    func stop(context: DictationContext,
              completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        queue.async { [weak self] in
            guard let self else { return }
            guard self.inSession, let stream = self.stream else {
                completion(.failure(.connectionFailed))
                return
            }
            Task { [weak self] in
                guard let self else { return }
                await stream.finalize()
                self.queue.async {
                    self.deliverResult(completion: completion)
                }
            }
        }
    }

    func cancel() {
        queue.async { [weak self] in
            guard let self else { return }
            self.stream?.close()
            self.stream = nil
            self.finals.removeAll()
            self.packetCount = 0
            self.failureMessage = nil
            self.inSession = false
        }
    }

    // MARK: - Session management

    private func beginSession() {
        finals.removeAll()
        packetCount = 0
        failureMessage = nil
        stream?.close()

        let token: ClaudeCodeOAuthToken
        do {
            token = try ClaudeCodeKeychain.read()
        } catch {
            wLog("Claude Voice: \(error)")
            stream = nil
            inSession = false
            return
        }
        if token.isExpired {
            wLog("Claude Voice: token expired — run `claude /login`")
            inSession = false
            return
        }

        // Keyterms: distil from user dictionary phrases. OCR-derived terms
        // can't be passed retroactively (they're in the URL query), so we
        // skip them in V1. Dictionary + names is the high-signal subset.
        var keytermLines: [String] = []
        if let phrases = dictionaryStore?.getVocabularyPhrases() {
            keytermLines.append(contentsOf: phrases)
        }
        let keyterms = ClaudeVoiceKeyTerms.extract(from: keytermLines, limit: 20)

        let language = settings.languages.first ?? "en"
        let config = VoiceStreamConfig(
            accessToken: token.accessToken,
            language: language,
            keyterms: keyterms
        )

        let voice = VoiceStream(config: config)
        voice.delegate = self
        stream = voice
        inSession = true
        do {
            try voice.connect()
        } catch {
            wLog("Claude Voice: failed to connect — \(error.localizedDescription)")
            stream = nil
            inSession = false
        }
    }

    private func deliverResult(completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        let collected: String
        finalsLock.lock()
        collected = finals.joined(separator: " ")
        finals.removeAll()
        finalsLock.unlock()

        let cleaned = collected.trimmingCharacters(in: .whitespacesAndNewlines)
        let packets = packetCount
        let failure = failureMessage

        inSession = false
        stream = nil
        packetCount = 0
        failureMessage = nil

        if let failure = failure, cleaned.isEmpty {
            completion(.failure(.serverError(failure)))
            return
        }
        guard !cleaned.isEmpty else {
            completion(.failure(.emptyResult))
            return
        }
        let duration = Double(packets) * Double(Constants.chunkDurationMs) / 1000.0
        let result = TranscriptResult(
            id: UUID().uuidString,
            asrText: cleaned,
            formattedText: cleaned,
            duration: duration,
            numWords: cleaned.split(separator: " ").count
        )
        completion(.success(result))
    }

    // MARK: - VoiceStreamDelegate

    func voiceStream(_ stream: VoiceStream, didReceiveInterim text: String) {
        // Lightning's pill doesn't render interim transcripts today; we only
        // collect finals. Keeping this hook so future UI can show partials.
    }

    func voiceStream(_ stream: VoiceStream, didReceiveFinal text: String) {
        finalsLock.lock()
        finals.append(text)
        finalsLock.unlock()
    }

    func voiceStream(_ stream: VoiceStream, didFailWith message: String, fatal: Bool) {
        wLog("Claude Voice: \(message) (fatal=\(fatal))")
        queue.async { [weak self] in
            self?.failureMessage = message
        }
    }

    func voiceStreamDidOpen(_ stream: VoiceStream) {
        wLog("Claude Voice: stream opened")
    }

    func voiceStreamDidClose(_ stream: VoiceStream) {
        wLogVerbose("Claude Voice: stream closed")
    }
}
