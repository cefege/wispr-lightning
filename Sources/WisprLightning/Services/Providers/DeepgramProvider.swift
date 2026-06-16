import Foundation

/// `DictationProvider` backed by Deepgram's `/v1/listen` streaming WebSocket.
/// Live PCM upload (same shape as Claude Voice) — packets are flushed live as
/// they arrive, finals accumulated as the server returns them, and a Finalize
/// message is sent on `stop()` to drain any pending audio before close.
///
/// Auth is a single BYO API key (env var or SecretsStore), same UX as
/// OpenRouter. Model is hard-coded to Nova-3 (Deepgram's flagship). Language
/// is user-selectable; the picker also exposes Deepgram's auto-detect
/// (`detect_language=true`) and multilingual code-switching (`language=multi`)
/// modes via sentinel values in `settings.deepgramLanguage`.
final class DeepgramProvider: NSObject, DictationProvider, URLSessionWebSocketDelegate {
    var dictionaryStore: DictionaryStore?

    private let settings: AppSettings
    private let queue = DispatchQueue(label: "WisprLightning.DeepgramProvider")

    private var urlSession: URLSession?
    private var task: URLSessionWebSocketTask?

    /// Packets that arrived before the WS handshake completed. AVAudioEngine
    /// emits its first packet ~150 ms after start, but the WS upgrade takes
    /// 700–1500 ms. Without this buffer the first ~1 s of speech is lost.
    /// Same gotcha that bit ClaudeVoiceProvider.
    private var bufferedPackets: [Data] = []
    private let bufferLock = NSLock()
    private var isOpen = false

    private var finalSegments: [String] = []
    private let finalsLock = NSLock()
    private var detectedLanguage: String?
    private var packetCount: Int = 0

    /// Set when beginSession or the server reports a failure. On `stop()` we
    /// surface this message instead of returning an empty transcript so the
    /// pill tells the user what to do (re-paste key, etc.).
    private var failureMessage: String?
    /// True when the failure should route as `.authFailed` (non-retryable +
    /// proper user message) instead of `.serverError`.
    private var failureIsAuth = false

    private var keepaliveTimer: DispatchSourceTimer?
    /// Used by the keepalive timer to skip sending when audio packets are
    /// already keeping the stream alive.
    private var lastSendAt: Date = .distantPast

    /// Single source of truth for "fire the stop() completion at most once."
    /// Replaces the previous pendingCompletion+didDeliverCompletion+nil-checks
    /// triad. Set in `stop()`, fired by whichever of finalize-success /
    /// connection-error / finalize-timeout wins the race, cleared in
    /// `beginSession()` for the next dictation.
    private var completionGate: SafeCompletion<Result<TranscriptResult, TranscriptionError>>?
    private var waitingForFinalize = false
    private var finalizeWaitItem: DispatchWorkItem?

    init(settings: AppSettings) {
        self.settings = settings
        super.init()
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
            if self.isOpen, let task = self.task {
                task.send(.data(packet)) { _ in }
                self.lastSendAt = Date()
            } else {
                self.bufferLock.lock()
                self.bufferedPackets.append(packet)
                self.bufferLock.unlock()
            }
        }
    }

    func stop(context: DictationContext,
              completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        queue.async { [weak self] in
            guard let self else { return }
            // beginSession failed before we ever opened a task (no API key,
            // malformed URL). Surface the recorded reason via the right
            // error category so the chain advances correctly.
            guard let task = self.task else {
                let msg = self.failureMessage ?? "Deepgram is not configured."
                let isAuth = self.failureIsAuth
                self.failureMessage = nil
                self.failureIsAuth = false
                if isAuth {
                    completion(.failure(.authFailed(msg)))
                } else {
                    completion(.failure(.serverError(msg)))
                }
                return
            }

            self.completionGate = SafeCompletion { result in
                completion(result)
            }
            self.waitingForFinalize = true

            // Flush pending audio: server returns one last Results with
            // is_final=true and from_finalize=true. We complete on receipt
            // of that frame OR after a 3s timeout, whichever comes first.
            task.send(.string(#"{"type":"Finalize"}"#)) { _ in }
            let item = DispatchWorkItem { [weak self] in
                self?.queue.async {
                    self?.completeAndClose()
                }
            }
            self.finalizeWaitItem = item
            self.queue.asyncAfter(deadline: .now() + 3.0, execute: item)
        }
    }

    func cancel() {
        queue.async { [weak self] in
            guard let self else { return }
            self.tearDown(reason: "cancel")
            self.completionGate = nil
            self.waitingForFinalize = false
            self.finalizeWaitItem?.cancel()
            self.finalizeWaitItem = nil
            self.failureMessage = nil
            self.failureIsAuth = false
            self.finalsLock.lock(); self.finalSegments.removeAll(); self.finalsLock.unlock()
            self.packetCount = 0
        }
    }

    // MARK: - Session

    private func beginSession() {
        finalsLock.lock(); finalSegments.removeAll(); finalsLock.unlock()
        bufferLock.lock(); bufferedPackets.removeAll(); bufferLock.unlock()
        isOpen = false
        packetCount = 0
        failureMessage = nil
        failureIsAuth = false
        detectedLanguage = nil
        completionGate = nil

        guard let apiKey = Self.apiKey() else {
            failureMessage = "Deepgram has no saved API key. Open Settings → Accounts → Deepgram and paste one from console.deepgram.com."
            failureIsAuth = true
            wLog("Deepgram: no API key — open Settings → Accounts → Deepgram")
            return
        }

        guard let url = buildURL() else {
            failureMessage = "Deepgram: failed to build URL"
            wLog("Deepgram: failed to build URL")
            return
        }

        var request = URLRequest(url: url)
        request.setValue("Token \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = 30

        let cfg = URLSessionConfiguration.default
        let session = URLSession(configuration: cfg, delegate: self, delegateQueue: nil)
        let wsTask = session.webSocketTask(with: request)
        self.urlSession = session
        self.task = wsTask
        wLog("Deepgram: connecting to \(url.host ?? "?") model=nova-3 language=\(settings.deepgramLanguage)")
        wsTask.resume()
        receiveLoop(task: wsTask)
        startKeepalive()
    }

    private func buildURL() -> URL? {
        guard var components = URLComponents(string: "wss://api.deepgram.com/v1/listen") else { return nil }
        var items: [URLQueryItem] = [
            URLQueryItem(name: "model", value: "nova-3"),
            URLQueryItem(name: "encoding", value: "linear16"),
            URLQueryItem(name: "sample_rate", value: String(Constants.sampleRate)),
            URLQueryItem(name: "channels", value: String(Constants.channels)),
            URLQueryItem(name: "smart_format", value: "true"),
            URLQueryItem(name: "punctuate", value: "true"),
            URLQueryItem(name: "interim_results", value: "false"),
            // Privacy: opt out of Deepgram's Model Improvement Program so
            // dictated audio isn't retained for training. No pricing impact
            // on the Nova-3 streaming tier.
            URLQueryItem(name: "mip_opt_out", value: "true"),
        ]

        switch settings.deepgramLanguage {
        case DeepgramLanguage.autoDetectCode:
            items.append(URLQueryItem(name: "detect_language", value: "true"))
        case DeepgramLanguage.multiCode:
            items.append(URLQueryItem(name: "language", value: "multi"))
        default:
            let code = settings.deepgramLanguage.isEmpty ? "en" : settings.deepgramLanguage
            items.append(URLQueryItem(name: "language", value: code))
        }

        // Keyterms — Nova-3 supports up to 500 tokens. Capped at 50 phrases
        // since the Wispr Flow / Claude Voice providers use the same shape.
        // Repeated `keyterm=…` so each phrase is boosted independently
        // rather than fused into one space-delimited cohesive unit.
        if let phrases = dictionaryStore?.getVocabularyPhrases() {
            for phrase in phrases.prefix(50) {
                items.append(URLQueryItem(name: "keyterm", value: phrase))
            }
        }
        components.queryItems = items
        return components.url
    }

    private func receiveLoop(task: URLSessionWebSocketTask) {
        task.receive { [weak self, weak task] result in
            guard let self else { return }
            switch result {
            case .failure(let error):
                self.queue.async {
                    self.handleConnectionError(error)
                }
            case .success(let message):
                self.queue.async {
                    self.handleMessage(message)
                }
                if let task = task {
                    self.receiveLoop(task: task)
                }
            }
        }
    }

    private func handleMessage(_ message: URLSessionWebSocketTask.Message) {
        switch message {
        case .string(let text):
            parseJSON(text)
        case .data(let data):
            if let text = String(data: data, encoding: .utf8) {
                parseJSON(text)
            }
        @unknown default:
            break
        }
    }

    private func parseJSON(_ text: String) {
        guard let data = text.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        let type = json["type"] as? String
        switch type {
        case "Results":
            guard let isFinal = json["is_final"] as? Bool, isFinal else { return }
            guard let channel = json["channel"] as? [String: Any],
                  let alternatives = channel["alternatives"] as? [[String: Any]],
                  let first = alternatives.first,
                  let transcript = first["transcript"] as? String,
                  !transcript.trimmingCharacters(in: .whitespaces).isEmpty else {
                // Empty is_final frame after Finalize — still triggers close.
                if let from = json["from_finalize"] as? Bool, from, waitingForFinalize {
                    finalizeWaitItem?.cancel()
                    finalizeWaitItem = nil
                    completeAndClose()
                }
                return
            }
            finalsLock.lock()
            finalSegments.append(transcript)
            finalsLock.unlock()
            // Capture detected language (auto-detect mode) or per-word
            // languages (multi mode) for logging.
            if detectedLanguage == nil {
                if let detected = channel["detected_language"] as? String {
                    detectedLanguage = detected
                } else if let langs = channel["languages"] as? [String], let first = langs.first {
                    detectedLanguage = first
                }
            }
            if let from = json["from_finalize"] as? Bool, from, waitingForFinalize {
                finalizeWaitItem?.cancel()
                finalizeWaitItem = nil
                completeAndClose()
            }
        case "Metadata":
            break
        default:
            break
        }
    }

    private func handleConnectionError(_ error: Error) {
        let response = task?.response as? HTTPURLResponse
        let statusCode = response?.statusCode ?? 0
        // A normal close after we've already delivered the result shouldn't
        // be reported as an error — that's just the server tearing down
        // after our CloseStream.
        if task == nil && completionGate == nil { return }
        wLog("Deepgram: WS error code=\(statusCode) — \(error.localizedDescription)")
        switch statusCode {
        case 401, 403:
            failureMessage = "Deepgram: API key was rejected (HTTP \(statusCode)). Open Settings → Accounts → Deepgram and paste a fresh key from console.deepgram.com."
            failureIsAuth = true
        case 400, 404:
            failureMessage = "Deepgram: bad request (HTTP \(statusCode)) — check model/language. \(error.localizedDescription)"
            failureIsAuth = true
        case 429:
            failureMessage = "Deepgram: rate limited (HTTP 429). \(error.localizedDescription)"
        case 500...599:
            failureMessage = "Deepgram: server error HTTP \(statusCode). \(error.localizedDescription)"
        default:
            failureMessage = "Deepgram: connection failed — \(error.localizedDescription)"
        }
        if let gate = completionGate {
            waitingForFinalize = false
            finalizeWaitItem?.cancel()
            finalizeWaitItem = nil
            if failureIsAuth {
                gate.fire(.failure(.authFailed(failureMessage)))
            } else if statusCode == 0 {
                gate.fire(.failure(.connectionFailed))
            } else {
                gate.fire(.failure(.serverError(failureMessage ?? "Deepgram error")))
            }
        }
        tearDown(reason: "error")
    }

    private func completeAndClose() {
        guard let gate = completionGate, !gate.hasCompleted else { return }
        waitingForFinalize = false
        finalizeWaitItem = nil

        task?.send(.string(#"{"type":"CloseStream"}"#)) { _ in }
        // Give the server ~200ms to flush its close frame before we cancel.
        // Tearing down too early can manifest as a spurious WS error in the
        // receive loop.
        queue.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            self?.tearDown(reason: "complete")
        }

        let collected: String
        finalsLock.lock()
        collected = finalSegments.joined(separator: " ")
        finalSegments.removeAll()
        finalsLock.unlock()

        let cleaned = collected.trimmingCharacters(in: .whitespacesAndNewlines)
        let packets = packetCount
        packetCount = 0

        if cleaned.isEmpty {
            if let msg = failureMessage {
                if failureIsAuth { gate.fire(.failure(.authFailed(msg))) }
                else { gate.fire(.failure(.serverError(msg))) }
            } else {
                gate.fire(.failure(.emptyResult))
            }
            return
        }
        let duration = Double(packets) * Double(Constants.chunkDurationMs) / 1000.0
        if let detected = detectedLanguage {
            wLog("Deepgram: detected_language=\(detected), got \(cleaned.count) chars, \(String(format: "%.1f", duration))s")
        } else {
            wLog("Deepgram: got \(cleaned.count) chars, \(String(format: "%.1f", duration))s")
        }
        let result = TranscriptResult(
            id: UUID().uuidString,
            asrText: cleaned,
            formattedText: cleaned,
            duration: duration,
            numWords: cleaned.split(separator: " ").count
        )
        gate.fire(.success(result))
    }

    private func tearDown(reason: String) {
        stopKeepalive()
        task?.cancel(with: .normalClosure, reason: reason.data(using: .utf8))
        task = nil
        urlSession?.invalidateAndCancel()
        urlSession = nil
        isOpen = false
        bufferLock.lock(); bufferedPackets.removeAll(); bufferLock.unlock()
    }

    // MARK: - KeepAlive

    private func startKeepalive() {
        stopKeepalive()
        let timer = DispatchSource.makeTimerSource(queue: queue)
        // 5s cadence is well inside Deepgram's 10s idle window. The handler
        // skips the actual send when a packet was sent recently — during
        // active dictation this timer is a no-op.
        timer.schedule(deadline: .now() + 5, repeating: 5)
        timer.setEventHandler { [weak self] in
            guard let self else { return }
            if Date().timeIntervalSince(self.lastSendAt) >= 4.5 {
                self.task?.send(.string(#"{"type":"KeepAlive"}"#)) { _ in }
            }
        }
        timer.resume()
        keepaliveTimer = timer
    }

    private func stopKeepalive() {
        keepaliveTimer?.cancel()
        keepaliveTimer = nil
    }

    // MARK: - Helpers

    private static func apiKey() -> String? {
        if let env = ProcessInfo.processInfo.environment["WISPR_LIGHTNING_DEEPGRAM_KEY"],
           !env.isEmpty {
            return env
        }
        return SecretsStore.read(.deepgramAPIKey)
    }

    // MARK: - URLSessionWebSocketDelegate

    func urlSession(_ session: URLSession,
                    webSocketTask: URLSessionWebSocketTask,
                    didOpenWithProtocol protocol: String?) {
        queue.async { [weak self] in
            guard let self else { return }
            wLog("Deepgram: stream opened")
            self.isOpen = true
            self.bufferLock.lock()
            let flushed = self.bufferedPackets
            self.bufferedPackets.removeAll(keepingCapacity: true)
            self.bufferLock.unlock()
            for packet in flushed {
                webSocketTask.send(.data(packet)) { _ in }
            }
            if !flushed.isEmpty {
                self.lastSendAt = Date()
            }
        }
    }

    func urlSession(_ session: URLSession,
                    webSocketTask: URLSessionWebSocketTask,
                    didCloseWith closeCode: URLSessionWebSocketTask.CloseCode,
                    reason: Data?) {
        queue.async { [weak self] in
            guard let self else { return }
            let reasonStr = reason.flatMap { String(data: $0, encoding: .utf8) } ?? "(none)"
            wLogVerbose("Deepgram: stream closed code=\(closeCode.rawValue) reason=\(reasonStr)")
            self.isOpen = false
        }
    }
}

// MARK: - Language list

/// Languages exposed in the Deepgram settings picker, plus the two sentinel
/// modes (auto-detect and multilingual code-switching). The BCP-47 entries
/// mirror Deepgram's streaming language-detection set.
enum DeepgramLanguage {
    struct Entry: Identifiable {
        let code: String
        let name: String
        var id: String { code }
    }

    static let autoDetectCode = "__auto__"
    static let multiCode = "__multi__"
    static let defaultCode = "en"

    /// Sorted alphabetically by English name.
    static let entries: [Entry] = [
        Entry(code: "bg",    name: "Bulgarian"),
        Entry(code: "ca",    name: "Catalan"),
        Entry(code: "zh",    name: "Chinese"),
        Entry(code: "cs",    name: "Czech"),
        Entry(code: "da",    name: "Danish"),
        Entry(code: "nl",    name: "Dutch"),
        Entry(code: "en",    name: "English"),
        Entry(code: "et",    name: "Estonian"),
        Entry(code: "fi",    name: "Finnish"),
        Entry(code: "nl-BE", name: "Flemish"),
        Entry(code: "fr",    name: "French"),
        Entry(code: "de",    name: "German"),
        Entry(code: "de-CH", name: "German (Switzerland)"),
        Entry(code: "el",    name: "Greek"),
        Entry(code: "hi",    name: "Hindi"),
        Entry(code: "hu",    name: "Hungarian"),
        Entry(code: "id",    name: "Indonesian"),
        Entry(code: "it",    name: "Italian"),
        Entry(code: "ja",    name: "Japanese"),
        Entry(code: "ko",    name: "Korean"),
        Entry(code: "lv",    name: "Latvian"),
        Entry(code: "lt",    name: "Lithuanian"),
        Entry(code: "ms",    name: "Malay"),
        Entry(code: "no",    name: "Norwegian"),
        Entry(code: "pl",    name: "Polish"),
        Entry(code: "pt",    name: "Portuguese"),
        Entry(code: "ro",    name: "Romanian"),
        Entry(code: "ru",    name: "Russian"),
        Entry(code: "sk",    name: "Slovak"),
        Entry(code: "es",    name: "Spanish"),
        Entry(code: "sv",    name: "Swedish"),
        Entry(code: "th",    name: "Thai"),
        Entry(code: "tr",    name: "Turkish"),
        Entry(code: "uk",    name: "Ukrainian"),
        Entry(code: "vi",    name: "Vietnamese"),
    ]

    static func displayName(for code: String) -> String {
        switch code {
        case autoDetectCode: return "Auto-detect"
        case multiCode: return "Multilingual (code-switching)"
        default:
            if let entry = entries.first(where: { $0.code == code }) {
                return "\(entry.name) (\(entry.code))"
            }
            return code
        }
    }
}
