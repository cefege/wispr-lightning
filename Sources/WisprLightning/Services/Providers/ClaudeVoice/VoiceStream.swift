import Foundation

/// Configuration for one streaming session against Claude Code's STT WebSocket.
struct VoiceStreamConfig {
    var baseURL: URL
    var accessToken: String
    var language: String
    var keyterms: [String]
    var userAgent: String
    var appHeader: String

    init(
        baseURL: URL = VoiceStreamConfig.defaultBaseURL,
        accessToken: String,
        language: String = "en",
        keyterms: [String] = [],
        userAgent: String = "wispr-lightning/0.1 (macOS)",
        appHeader: String = "cli"
    ) {
        self.baseURL = baseURL
        self.accessToken = accessToken
        self.language = language
        self.keyterms = keyterms
        self.userAgent = userAgent
        self.appHeader = appHeader
    }

    /// api.anthropic.com is the registered API surface and matches the verified
    /// Claude Code 2.1.119 binary behavior. Override via env for dev.
    static var defaultBaseURL: URL {
        if let override = ProcessInfo.processInfo.environment["VOICE_STREAM_BASE_URL"],
           let url = URL(string: override) {
            return url
        }
        return URL(string: "wss://api.anthropic.com")!
    }

    private static let endpointPath = "/api/ws/speech_to_text/voice_stream"

    func buildURL() throws -> URL {
        var components = URLComponents()
        components.scheme = baseURL.scheme == "wss" ? "wss" : "ws"
        components.host = baseURL.host
        if let port = baseURL.port { components.port = port }
        components.path = Self.endpointPath

        var items: [URLQueryItem] = [
            .init(name: "encoding", value: "linear16"),
            .init(name: "sample_rate", value: String(Constants.sampleRate)),
            .init(name: "channels", value: "1"),
            .init(name: "endpointing_ms", value: "300"),
            .init(name: "utterance_end_ms", value: "1000"),
            .init(name: "language", value: language),
            .init(name: "use_conversation_engine", value: "true"),
            .init(name: "stt_provider", value: "deepgram-nova3"),
        ]
        for term in keyterms { items.append(.init(name: "keyterms", value: term)) }
        components.queryItems = items

        guard let url = components.url else {
            throw NSError(domain: "VoiceStream", code: 2,
                          userInfo: [NSLocalizedDescriptionKey: "Failed to build URL"])
        }
        return url
    }
}

protocol VoiceStreamDelegate: AnyObject {
    func voiceStream(_ stream: VoiceStream, didReceiveInterim text: String)
    func voiceStream(_ stream: VoiceStream, didReceiveFinal text: String)
    func voiceStream(_ stream: VoiceStream, didFailWith message: String, fatal: Bool)
    func voiceStreamDidClose(_ stream: VoiceStream)
    func voiceStreamDidOpen(_ stream: VoiceStream)
}

/// 8s keepalive — verified from Claude Code binary 2.1.119. Wispr Flow uses 20s
/// for its load balancer; this is a different endpoint. Don't bump.
private let claudeVoiceKeepAliveInterval: TimeInterval = 8

final class VoiceStream: NSObject, URLSessionWebSocketDelegate {
    private let config: VoiceStreamConfig
    weak var delegate: VoiceStreamDelegate?

    private var session: URLSession!
    private var task: URLSessionWebSocketTask?
    private var keepAliveTimer: DispatchSourceTimer?

    private var isOpen = false
    private var didCloseStream = false
    private var lastInterim: String = ""
    private var pendingFinalization: CheckedContinuation<Void, Never>?
    /// Why: AVAudioEngine starts producing packets ~150ms after `start()`, but
    /// the WS open (TCP + TLS + Upgrade) takes 700-1500ms. Without buffering
    /// here, the first ~1s of speech is dropped on the floor before isOpen
    /// flips true, and the server never sees enough audio to emit a final.
    private var preOpenBuffer: [Data] = []

    private let queue = DispatchQueue(label: "WisprLightning.VoiceStream.queue")

    init(config: VoiceStreamConfig) {
        self.config = config
        super.init()
        let cfg = URLSessionConfiguration.default
        self.session = URLSession(configuration: cfg, delegate: self, delegateQueue: nil)
    }

    func connect() throws {
        let url = try config.buildURL()
        var request = URLRequest(url: url)
        request.setValue("Bearer \(config.accessToken)", forHTTPHeaderField: "Authorization")
        request.setValue(config.userAgent, forHTTPHeaderField: "User-Agent")
        request.setValue(config.appHeader, forHTTPHeaderField: "x-app")
        // Required by the Claude Code CLI variant of the endpoint switch per
        // verified binary 2.1.119. Missing this header returns a 4xx.
        request.setValue("claude_code_cli", forHTTPHeaderField: "anthropic-client-platform")

        wLog("Claude Voice: connecting to \(url.absoluteString)")

        let task = session.webSocketTask(with: request)
        self.task = task
        task.resume()
        receiveLoop()
    }

    func send(pcm: Data) {
        queue.async { [weak self] in
            guard let self, let task = self.task, !self.didCloseStream else { return }
            if self.isOpen {
                task.send(.data(pcm)) { error in
                    if let error {
                        wLogVerbose("Claude Voice: send error — \(error.localizedDescription)")
                    }
                }
            } else {
                // WS handshake still in flight — buffer until didOpenWithProtocol
                // fires, then flush in order. Without this, the first ~1s of
                // audio gets dropped silently and the server emits no transcript.
                self.preOpenBuffer.append(pcm)
            }
        }
    }

    /// Send CloseStream and wait briefly for a TranscriptEndpoint, then close.
    /// Tolerant of being called before the socket opens — just closes silently.
    func finalize() async {
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            queue.async { [weak self] in
                guard let self else { cont.resume(); return }
                guard let task = self.task else {
                    cont.resume()
                    return
                }
                self.didCloseStream = true
                if self.isOpen {
                    task.send(.string(#"{"type":"CloseStream"}"#)) { _ in }
                }
                self.pendingFinalization = cont
                DispatchQueue.global().asyncAfter(deadline: .now() + 2.0) {
                    self.queue.async { self.resolveFinalization() }
                }
            }
        }
    }

    private func resolveFinalization() {
        guard let cont = pendingFinalization else { return }
        pendingFinalization = nil
        flushLastInterimAsFinal()
        close()
        cont.resume()
    }

    func close() {
        keepAliveTimer?.cancel()
        keepAliveTimer = nil
        task?.cancel(with: .normalClosure, reason: nil)
        task = nil
        isOpen = false
        preOpenBuffer.removeAll(keepingCapacity: false)
    }

    // MARK: - URLSessionWebSocketDelegate

    func urlSession(_ session: URLSession,
                    webSocketTask: URLSessionWebSocketTask,
                    didOpenWithProtocol protocolStr: String?) {
        queue.async { [weak self] in
            guard let self else { return }
            self.isOpen = true
            self.startKeepAlive()
            self.delegate?.voiceStreamDidOpen(self)
            webSocketTask.send(.string(#"{"type":"KeepAlive"}"#)) { _ in }
            // Flush any packets that arrived during the handshake. Order is
            // preserved because we append on the same queue we send on.
            let buffered = self.preOpenBuffer
            self.preOpenBuffer.removeAll(keepingCapacity: false)
            if !buffered.isEmpty {
                wLog("Claude Voice: flushing \(buffered.count) pre-open packets")
                for pcm in buffered {
                    webSocketTask.send(.data(pcm)) { _ in }
                }
            }
        }
    }

    func urlSession(_ session: URLSession,
                    webSocketTask: URLSessionWebSocketTask,
                    didCloseWith closeCode: URLSessionWebSocketTask.CloseCode,
                    reason: Data?) {
        queue.async { [weak self] in
            guard let self else { return }
            self.flushLastInterimAsFinal()
            self.isOpen = false
            self.keepAliveTimer?.cancel()
            self.keepAliveTimer = nil
            self.delegate?.voiceStreamDidClose(self)
            wLog("Claude Voice: didCloseWith code=\(closeCode.rawValue) reason=\(Self.describeCloseReason(reason))")
            if let message = Self.closeFailureMessage(code: closeCode, reason: reason) {
                self.delegate?.voiceStream(self, didFailWith: message, fatal: false)
            }
            self.resolveFinalization()
        }
    }

    private static func closeFailureMessage(
        code: URLSessionWebSocketTask.CloseCode,
        reason: Data?
    ) -> String? {
        if code == .normalClosure || code == .noStatusReceived { return nil }
        let reasonStr = describeCloseReason(reason)
        return "Closed with code \(code.rawValue) \(reasonStr)"
    }

    private static func describeCloseReason(_ reason: Data?) -> String {
        guard let data = reason, !data.isEmpty,
              let str = String(data: data, encoding: .utf8) else {
            return ""
        }
        return str
    }

    // MARK: - Internal

    private func startKeepAlive() {
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + claudeVoiceKeepAliveInterval, repeating: claudeVoiceKeepAliveInterval)
        timer.setEventHandler { [weak self] in
            guard let self, let task = self.task, self.isOpen else { return }
            task.send(.string(#"{"type":"KeepAlive"}"#)) { _ in }
        }
        timer.resume()
        keepAliveTimer = timer
    }

    private func receiveLoop() {
        task?.receive { [weak self] result in
            guard let self else { return }
            switch result {
            case .failure(let error):
                self.queue.async {
                    let action = Self.classifyReceiveFailure(
                        error: error,
                        didCloseStream: self.didCloseStream
                    )
                    switch action {
                    case .suppress:
                        wLogVerbose("Claude Voice: receive ended after clean close — \(error.localizedDescription)")
                    case .report(let message):
                        self.delegate?.voiceStream(self, didFailWith: message, fatal: true)
                    }
                    self.close()
                }
                return
            case .success(let message):
                self.handleMessage(message)
                self.receiveLoop()
            }
        }
    }

    enum ReceiveFailureAction: Equatable {
        case suppress
        case report(message: String)
    }

    private static func classifyReceiveFailure(
        error: Error,
        didCloseStream: Bool
    ) -> ReceiveFailureAction {
        if didCloseStream { return .suppress }
        if let urlError = error as? URLError, urlError.code == .cancelled {
            return .suppress
        }
        return .report(message: "Receive error: \(error.localizedDescription)")
    }

    private func handleMessage(_ message: URLSessionWebSocketTask.Message) {
        let text: String
        switch message {
        case .data(let d): text = String(data: d, encoding: .utf8) ?? ""
        case .string(let s): text = s
        @unknown default: return
        }
        guard
            let data = text.data(using: .utf8),
            let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return }

        let type = (obj["type"] as? String) ?? ""
        switch type {
        case "TranscriptText":
            if let t = obj["data"] as? String, !t.isEmpty {
                // lastInterim is consumed by resolveFinalization on `queue`;
                // updating it here on the URLSession delegate queue would
                // race with that read. Bounce onto `queue` to serialize.
                queue.async { [weak self] in self?.lastInterim = t }
                delegate?.voiceStream(self, didReceiveInterim: t)
            }
        case "TranscriptEndpoint":
            // flushLastInterimAsFinal mutates lastInterim, which is also
            // touched from `queue` by resolveFinalization. Bounce onto the
            // serial queue so both paths can't race for the final string.
            queue.async { [weak self] in
                self?.flushLastInterimAsFinal()
                self?.resolveFinalization()
            }
        case "TranscriptError":
            let msg = (obj["description"] as? String)
                ?? (obj["error_code"] as? String)
                ?? "unknown transcription error"
            delegate?.voiceStream(self, didFailWith: msg, fatal: false)
        case "error":
            let msg = (obj["message"] as? String) ?? "server error"
            delegate?.voiceStream(self, didFailWith: msg, fatal: false)
        default:
            break
        }
    }

    private func flushLastInterimAsFinal() {
        let pending = lastInterim
        lastInterim = ""
        if !pending.isEmpty {
            delegate?.voiceStream(self, didReceiveFinal: pending)
        }
    }
}
