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

    /// OCR / screen-context lines for the *next* session. Set by AppDelegate
    /// from whatever OCR finished during the previous recording — the WS URL
    /// fixes keyterms at connect-time, so we can't add them retroactively.
    /// First recording of the launch sees an empty list; subsequent recordings
    /// get keyterms distilled from the preceding session's screen capture.
    var pendingOcrLines: [String] = []
    private let hintLock = NSLock()
    func setPendingOcrLines(_ lines: [String]) {
        hintLock.lock()
        pendingOcrLines = lines
        hintLock.unlock()
    }

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
                // beginSession failed (no/expired token, or connect error).
                // Surface the recorded reason so the pill tells the user what to do.
                let msg = self.failureMessage ?? "Claude Voice is not signed in. Run `claude /login` in a terminal."
                self.failureMessage = nil
                completion(.failure(.serverError(msg)))
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
            failureMessage = "Run `claude /login` in a terminal, then try again."
            stream = nil
            inSession = false
            return
        }
        if token.isExpired {
            wLog("Claude Voice: token expired — run `claude /login`")
            failureMessage = "Claude Code token expired — run `claude /login`."
            inSession = false
            return
        }

        // Keyterms: combine user dictionary phrases with OCR lines from the
        // previous session (current session's OCR happens in parallel and
        // can't land in the URL retroactively). Dictionary phrases bypass
        // the NL tagger — they're already curated proper nouns — and are
        // appended to whatever the tagger distills from OCR.
        hintLock.lock()
        let ocrLines = pendingOcrLines
        hintLock.unlock()
        var keyterms = ClaudeVoiceKeyTerms.extract(from: ocrLines, limit: 20)
        if let phrases = dictionaryStore?.getVocabularyPhrases() {
            for phrase in phrases where !keyterms.contains(phrase) {
                keyterms.append(phrase)
                if keyterms.count >= 20 { break }
            }
        }

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
            // Tear the stream down so its URLSession (which retains the
            // VoiceStream as its delegate) doesn't leak.
            voice.close()
            stream = nil
            inSession = false
            failureMessage = "Failed to open Claude Voice stream: \(error.localizedDescription)"
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
