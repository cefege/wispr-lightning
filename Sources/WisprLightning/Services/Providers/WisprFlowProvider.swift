import Foundation

/// `DictationProvider` backed by Wispr Flow's WebSocket transcription API.
///
/// Internally buffers PCM packets fed via `feed(packet:)` and uploads them in
/// one WebSocket session at `stop(context:completion:)` time. The WebSocket
/// can be prewarmed during recording so TCP+TLS handshake overlaps with mic
/// startup.
class WisprFlowProvider: DictationProvider {
    private let session: Session
    private let settings: AppSettings
    var dictionaryStore: DictionaryStore?
    private var prewarmedTask: URLSessionWebSocketTask?
    private let prewarmLock = NSLock()
    /// Max packets per WebSocket append message (~20 seconds of audio, ~800KB encoded)
    private static let chunkSize = 500
    /// Keepalive ping interval. Well under typical 60s NAT / load-balancer idle timeouts
    /// so the socket stays open during long recordings (B-003: 90s recordings dropped).
    private static let pingInterval: TimeInterval = 20.0
    private var cachedEncoding: (packetCount: Int, prepared: PreparedAudio)?
    private let encodingQueue = DispatchQueue(label: "com.wisprlightning.encode", qos: .userInitiated)
    /// Tracks pinger work items so we can stop pinging when a task is handed off, cancelled, or closed.
    private var pingWorkItems: [ObjectIdentifier: DispatchWorkItem] = [:]
    private let pingLock = NSLock()

    private var bufferedPackets: [Data] = []
    private let bufferLock = NSLock()

    private struct PreparedAudio {
        let encodedPackets: [String]
        let volumes: [Double]
    }

    /// Dynamic response timeout: minimum 15s, scales with recording duration
    private static func responseTimeout(for packetCount: Int) -> Double {
        max(15.0, Double(packetCount) * Double(Constants.chunkDurationMs) / 1000.0 * 0.5)
    }

    init(session: Session, settings: AppSettings) {
        self.session = session
        self.settings = settings
    }

    // MARK: - DictationProvider lifecycle

    func start() {
        bufferLock.lock()
        bufferedPackets.removeAll(keepingCapacity: true)
        bufferLock.unlock()
    }

    func feed(packet: Data) {
        bufferLock.lock()
        bufferedPackets.append(packet)
        bufferLock.unlock()
    }

    func stop(context: DictationContext,
              completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        bufferLock.lock()
        let packets = bufferedPackets
        bufferedPackets.removeAll(keepingCapacity: false)
        bufferLock.unlock()
        transcribe(packets: packets, context: context, completion: completion)
    }

    func cancel() {
        bufferLock.lock()
        bufferedPackets.removeAll(keepingCapacity: false)
        bufferLock.unlock()
        cancelPrewarmedConnection()
    }

    // MARK: - Connection management

    private func createWebSocketTask() -> URLSessionWebSocketTask? {
        guard let url = URL(string: Constants.wsURL) else { return nil }
        var request = URLRequest(url: url)
        request.setValue("json", forHTTPHeaderField: "Encoding")
        let task = URLSession.shared.webSocketTask(with: request)
        task.maximumMessageSize = 10 * 1024 * 1024 // 10MB receive buffer
        task.resume()
        startPinging(task)
        return task
    }

    /// Schedules a recurring WebSocket ping every `pingInterval` seconds while the task is open.
    /// Stops automatically once the task leaves the `.running` state or `stopPinging` is called.
    /// `sendPing` errors are logged but otherwise swallowed — by then the socket is already closed
    /// and the next send/receive will surface the real error to the caller.
    private func startPinging(_ task: URLSessionWebSocketTask) {
        let key = ObjectIdentifier(task)
        var work: DispatchWorkItem!
        work = DispatchWorkItem { [weak self, weak task] in
            guard let self = self, let task = task else { return }
            guard task.state == .running else {
                self.stopPinging(task)
                return
            }
            self.pingLock.lock()
            let stillTracked = self.pingWorkItems[key] === work
            self.pingLock.unlock()
            guard stillTracked else { return }

            task.sendPing { error in
                if let error = error {
                    wLogVerbose("WS ping failed: \(error.localizedDescription)")
                }
            }
            DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + Self.pingInterval, execute: work)
        }
        pingLock.lock()
        pingWorkItems[key] = work
        pingLock.unlock()
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + Self.pingInterval, execute: work)
    }

    private func stopPinging(_ task: URLSessionWebSocketTask) {
        let key = ObjectIdentifier(task)
        pingLock.lock()
        let work = pingWorkItems.removeValue(forKey: key)
        pingLock.unlock()
        work?.cancel()
    }

    /// Start TCP+TLS handshake early so it's ready when audio finishes
    func prewarmConnection() {
        guard let task = createWebSocketTask() else { return }
        prewarmLock.lock()
        prewarmedTask = task
        prewarmLock.unlock()

        if !session.isValid {
            session.refresh { success in
                if !success {
                    NSLog("Wispr Lightning: Proactive token refresh failed")
                }
            }
        }
    }

    func cancelPrewarmedConnection() {
        prewarmLock.lock()
        let task = prewarmedTask
        prewarmedTask = nil
        prewarmLock.unlock()
        if let task = task {
            stopPinging(task)
            task.cancel(with: .normalClosure, reason: nil)
        }
    }

    // MARK: - Transcription pipeline

    /// Transcribe a packet array directly. Used by `stop()` after draining the
    /// internal buffer; the prior call site in `AppDelegate` no longer needs it.
    private func transcribe(packets: [Data],
                            context: DictationContext,
                            completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        guard !packets.isEmpty else {
            completion(.failure(.emptyResult))
            return
        }

        guard session.isValid else {
            NSLog("Wispr Lightning: Token expired, refreshing...")
            session.refresh { [weak self] success in
                guard success, let self = self else {
                    NSLog("Wispr Lightning: Cannot transcribe — auth failed")
                    // Broadcast on main so the status-bar observer (which
                    // posts to .main queue) can rebuild its menu without
                    // cross-queue UI updates.
                    DispatchQueue.main.async {
                        NotificationCenter.default.post(name: .sessionChanged, object: nil)
                    }
                    completion(.failure(.authFailed("Wispr Flow sign-in expired and refresh failed. Open Settings → Accounts → Wispr Flow and sign in again.")))
                    return
                }
                self.performTranscription(packets: packets, context: context, completion: completion)
            }
            return
        }

        performTranscription(packets: packets, context: context, completion: completion)
    }

    private func performTranscription(packets: [Data],
                                      context: DictationContext,
                                      completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        let gate = SafeCompletion<Result<TranscriptResult, TranscriptionError>> { result in
            completion(result)
        }
        // Alias so the existing `safeComplete(...)` call sites in this
        // function don't need touching. (Inlining everywhere would be a
        // mechanical churn; this preserves diff hygiene.)
        let safeComplete = gate.fire

        let wsTask: URLSessionWebSocketTask
        prewarmLock.lock()
        let prewarmed = prewarmedTask
        prewarmedTask = nil
        prewarmLock.unlock()
        if let prewarmed = prewarmed, prewarmed.state == .running {
            wsTask = prewarmed
        } else {
            if let prewarmed = prewarmed {
                wLog("Prewarmed connection stale (state: \(prewarmed.state.rawValue)), creating fresh one")
                stopPinging(prewarmed)
                prewarmed.cancel(with: .normalClosure, reason: nil)
            }
            guard let newTask = createWebSocketTask() else {
                safeComplete(.failure(.connectionFailed))
                return
            }
            wsTask = newTask
        }

        let transcriptUUID = UUID().uuidString
        let appInfo = context.appInfo
        let appType = (appInfo["type"] ?? "other").lowercased()

        let pipeline = settings.aiFormatting ? ["transcribe", "format"] : ["transcribe"]
        let authMsg: [String: Any] = [
            "type": "auth",
            "access_token": session.accessToken ?? "",
            "app": appType,
            "context": [
                "app": [
                    "name": appInfo["name"] ?? "",
                    "bundle_id": appInfo["bundle_id"] ?? "",
                    "type": appType,
                    "url": appInfo["url"] ?? ""
                ],
                "ax_context": context.axContext,
                "ocr_context": context.ocrContext,
                "dictionary_context": (dictionaryStore?.getVocabularyPhrases() ?? []) as Any,
                "dictionary_replacements": (dictionaryStore?.getReplacements() ?? [:]) as Any,
                "dictionary_snippets": (dictionaryStore?.getSnippets() ?? [:]).mapValues { [$0] } as Any,
                "user_first_name": session.userFirstName ?? "",
                "user_last_name": session.userLastName ?? "",
                "textbox_contents": [:] as [String: Any],
                "content_text": "",
                "variable_names": [] as [Any],
                "file_names": [] as [Any]
            ] as [String: Any],
            "personalization_style_settings": settings.styleDetectionEnabled ? settings.personalizationStyles : [:] as [String: String],
            "language": settings.languages,
            "metadata": [
                "session_id": session.sessionId,
                "environment": "PRODUCTION",
                "client_platform": "darwin",
                "client_version": Constants.clientVersion,
                "transcript_entity_uuid": transcriptUUID
            ] as [String: Any],
            "pipeline": pipeline,
            "job_selectors": (settings.creatorMode ? ["creator"] : []) as [Any],
            "cleanup_level": settings.autoCleanupLevel,
            "command_mode": settings.commandModeEnabled,
            "debug_mode": false,
            "use_staging_baseten": false,
            "prefix_is_written": !context.axContext.isEmpty,
            "hyperlink_on": settings.hyperlinkOn
        ]

        guard let authData = try? JSONSerialization.data(withJSONObject: authMsg),
              let authString = String(data: authData, encoding: .utf8) else {
            stopPinging(wsTask)
            wsTask.cancel(with: .internalServerError, reason: nil)
            safeComplete(.failure(.connectionFailed))
            return
        }

        var preparedAudio: PreparedAudio?
        var encodeGroup: DispatchGroup?

        if let cached = cachedEncoding, cached.packetCount == packets.count {
            preparedAudio = cached.prepared
        } else {
            let group = DispatchGroup()
            group.enter()
            encodingQueue.async {
                preparedAudio = self.prepareAudio(packets: packets)
                if let prepared = preparedAudio {
                    self.cachedEncoding = (packetCount: packets.count, prepared: prepared)
                }
                group.leave()
            }
            encodeGroup = group
        }

        wLogVerbose("WS sending auth — token: \(String((session.accessToken ?? "").prefix(8)))..., app: \(appType), pipeline: \(pipeline.joined(separator: ","))")

        wsTask.send(.string(authString)) { error in
            if let error = error {
                NSLog("Wispr Lightning: WS auth send failed: %@", error.localizedDescription)
                safeComplete(.failure(.connectionFailed))
                return
            }
        }

        // Auth timeout: without this, a hung server (upgrade succeeded but no
        // auth response) parks the recording in Processing until URLSession's
        // ~30s default resource timeout. That's user-visible as a stall with
        // no fallback. 10s is well past normal handshake (~700ms) but short
        // enough that the chain advances quickly when the backend is broken.
        let authTimeout = DispatchWorkItem { [weak self] in
            wLog("Wispr Flow: auth response timed out — falling back")
            self?.stopPinging(wsTask)
            wsTask.cancel(with: .goingAway, reason: nil)
            safeComplete(.failure(.timeout))
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + 10.0, execute: authTimeout)

        wsTask.receive { [weak self] result in
            // Cancel the auth-timeout watchdog as soon as we get any response
            // (success or failure). The safeComplete wrapper de-dupes if the
            // timeout already fired.
            authTimeout.cancel()
            guard let self = self else {
                safeComplete(.failure(.connectionFailed))
                return
            }
            switch result {
            case .success(let message):
                if case .string(let text) = message,
                   let data = text.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                    let statusWord = json["status"] as? String ?? "unknown"
                    wLog("WS auth response: status=\(statusWord)")
                    wLogVerbose("WS auth response full: \(text)")
                    if statusWord == "auth" {
                        wLog("WebSocket authenticated")
                        let sendAudio = {
                            self.sendPreparedAudio(wsTask: wsTask, prepared: preparedAudio, packetCount: packets.count, transcriptUUID: transcriptUUID, completion: safeComplete)
                        }
                        if let group = encodeGroup {
                            group.notify(queue: self.encodingQueue, execute: sendAudio)
                        } else {
                            sendAudio()
                        }
                    } else {
                        wLog("WebSocket auth failed — unexpected response")
                        self.stopPinging(wsTask)
                        wsTask.cancel(with: .internalServerError, reason: nil)
                        safeComplete(.failure(.authFailed("Wispr Flow rejected the WebSocket auth. Open Settings → Accounts → Wispr Flow and sign in again.")))
                    }
                } else {
                    wLog("WebSocket auth failed — non-string message received")
                    self.stopPinging(wsTask)
                    wsTask.cancel(with: .internalServerError, reason: nil)
                    safeComplete(.failure(.authFailed("Wispr Flow rejected the WebSocket auth. Open Settings → Accounts → Wispr Flow and sign in again.")))
                }
            case .failure(let error):
                wLog("WS receive failed: \(error.localizedDescription)")
                safeComplete(.failure(.connectionFailed))
            }
        }
    }

    private func prepareAudio(packets: [Data]) -> PreparedAudio? {
        var encodedPackets: [String] = []
        encodedPackets.reserveCapacity(packets.count)
        var volumes: [Double] = []
        volumes.reserveCapacity(packets.count)

        for packet in packets {
            encodedPackets.append(ascii85Encode(packet))

            let sampleCount = packet.count / 2
            var sumSquares: Double = 0
            packet.withUnsafeBytes { rawBuffer in
                let samples = rawBuffer.bindMemory(to: Int16.self)
                for i in 0..<sampleCount {
                    let s = Double(samples[i])
                    sumSquares += s * s
                }
            }
            let rms = (sumSquares / Double(sampleCount)).squareRoot()
            volumes.append((rms / 32768.0 * 10000).rounded() / 10000)
        }

        return PreparedAudio(encodedPackets: encodedPackets, volumes: volumes)
    }

    private func sendPreparedAudio(wsTask: URLSessionWebSocketTask, prepared: PreparedAudio?, packetCount: Int, transcriptUUID: String, completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        guard let prepared = prepared else {
            stopPinging(wsTask)
            wsTask.cancel(with: .internalServerError, reason: nil)
            completion(.failure(.connectionFailed))
            return
        }

        let totalPackets = prepared.encodedPackets.count
        wLog("Sending \(totalPackets) packets in chunks of \(Self.chunkSize)")
        sendNextChunk(wsTask: wsTask, prepared: prepared, offset: 0, totalPackets: totalPackets, transcriptUUID: transcriptUUID, completion: completion)
    }

    private func sendCommitAndReceive(wsTask: URLSessionWebSocketTask, totalPackets: Int, transcriptUUID: String, completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        let commitMsg: [String: Any] = [
            "type": "commit",
            "total_packets": totalPackets
        ]
        guard let commitData = try? JSONSerialization.data(withJSONObject: commitMsg),
              let commitString = String(data: commitData, encoding: .utf8) else {
            completion(.failure(.connectionFailed))
            return
        }

        wsTask.send(.string(commitString)) { [weak self] error in
            if let error = error {
                NSLog("Wispr Lightning: WS commit send failed: %@", error.localizedDescription)
                completion(.failure(.connectionFailed))
                return
            }
            // If the provider was deallocated mid-send, the WS task and the
            // completion handler are still alive — fail the in-flight result
            // rather than crash dereferencing nil.
            guard let self = self else {
                completion(.failure(.connectionFailed))
                return
            }

            let chunkCount = (totalPackets + Self.chunkSize - 1) / Self.chunkSize
            NSLog("Wispr Lightning: Audio sent — %d packets in %d chunks, waiting for transcription...", totalPackets, chunkCount)
            self.receiveResultWithTimeout(wsTask: wsTask, transcriptUUID: transcriptUUID, packetCount: totalPackets, completion: completion)
        }
    }

    private func sendNextChunk(wsTask: URLSessionWebSocketTask, prepared: PreparedAudio, offset: Int, totalPackets: Int, transcriptUUID: String, completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        let end = min(offset + Self.chunkSize, totalPackets)
        let isFinal = end >= totalPackets
        let chunkPackets = Array(prepared.encodedPackets[offset..<end])
        let chunkVolumes = Array(prepared.volumes[offset..<end])

        let appendMsg: [String: Any] = [
            "type": "append",
            "audio_packets": [
                "packets": chunkPackets,
                "volumes": chunkVolumes,
                "packet_duration": Double(Constants.chunkDurationMs) / 1000.0,
                "audio_encoding": "wav",
                "byte_encoding": "ascii85"
            ] as [String: Any],
            "position": offset,
            "final": isFinal
        ]

        guard let appendData = try? JSONSerialization.data(withJSONObject: appendMsg),
              let appendString = String(data: appendData, encoding: .utf8) else {
            completion(.failure(.connectionFailed))
            return
        }

        wLogVerbose("WS sending chunk \(offset)..<\(end) of \(totalPackets) (\(appendString.count) bytes, final=\(isFinal))")
        wsTask.send(.string(appendString)) { [weak self] error in
            if let error = error {
                NSLog("Wispr Lightning: WS chunk send failed: %@", error.localizedDescription)
                completion(.failure(.connectionFailed))
                return
            }
            // Same guard as commit-send: provider may be gone if the user
            // cancelled mid-upload. Fail the result cleanly instead of
            // crashing on a strong self.
            guard let self = self else {
                completion(.failure(.connectionFailed))
                return
            }

            if isFinal {
                self.sendCommitAndReceive(wsTask: wsTask, totalPackets: totalPackets, transcriptUUID: transcriptUUID, completion: completion)
            } else {
                self.sendNextChunk(wsTask: wsTask, prepared: prepared, offset: end, totalPackets: totalPackets, transcriptUUID: transcriptUUID, completion: completion)
            }
        }
    }

    private func receiveResultWithTimeout(wsTask: URLSessionWebSocketTask, transcriptUUID: String, packetCount: Int, completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        var completed = false
        let completionLock = NSLock()

        let safeComplete: (Result<TranscriptResult, TranscriptionError>) -> Void = { result in
            completionLock.lock()
            guard !completed else {
                completionLock.unlock()
                return
            }
            completed = true
            completionLock.unlock()
            completion(result)
        }

        let timeout = Self.responseTimeout(for: packetCount)
        let timeoutWork = DispatchWorkItem { [weak self] in
            NSLog("Wispr Lightning: WebSocket response timed out after %.0fs", timeout)
            self?.stopPinging(wsTask)
            wsTask.cancel(with: .abnormalClosure, reason: nil)
            safeComplete(.failure(.timeout))
        }
        DispatchQueue.global(qos: .userInitiated).asyncAfter(
            deadline: .now() + timeout,
            execute: timeoutWork
        )

        receiveResult(wsTask: wsTask, transcriptUUID: transcriptUUID, packetCount: packetCount) { result in
            timeoutWork.cancel()
            safeComplete(result)
        }
    }

    private func receiveResult(wsTask: URLSessionWebSocketTask, transcriptUUID: String, packetCount: Int, completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        wsTask.receive { [weak self] result in
            switch result {
            case .success(let message):
                if case .string(let text) = message,
                   let data = text.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {

                    wLogVerbose("WS received: \(text.prefix(500))")
                    let status = json["status"] as? String
                    if status == "text" {
                        let body = json["body"] as? [String: Any] ?? [:]
                        let llmText = body["llm_text"] as? String
                        let asrText = body["asr_text"] as? String
                        let isFinal = json["final"] as? Bool ?? false
                        let resultText = llmText ?? asrText ?? ""

                        NSLog("Wispr Lightning: Got %@ transcript: %d chars",
                              isFinal ? "final" : "partial", resultText.count)

                        if isFinal {
                            let duration = Double(packetCount) * Double(Constants.chunkDurationMs) / 1000.0
                            let wordCount = resultText.split(separator: " ").count
                            let transcriptResult = TranscriptResult(
                                id: transcriptUUID,
                                asrText: asrText,
                                formattedText: llmText,
                                duration: duration,
                                numWords: wordCount
                            )
                            self?.stopPinging(wsTask)
                            wsTask.cancel(with: .normalClosure, reason: nil)
                            if resultText.isEmpty {
                                completion(.failure(.emptyResult))
                            } else {
                                completion(.success(transcriptResult))
                            }
                            return
                        }
                    } else if status == "error" {
                        let errorDetail = json["error"] as? String ?? "unknown"
                        NSLog("Wispr Lightning: Server error: %@", errorDetail)
                        self?.stopPinging(wsTask)
                        wsTask.cancel(with: .internalServerError, reason: nil)
                        completion(.failure(.serverError(errorDetail)))
                        return
                    } else if status == "info" {
                        NSLog("Wispr Lightning: Server info: %@", json["message"] as? String ?? "")
                    }

                    self?.receiveResult(wsTask: wsTask, transcriptUUID: transcriptUUID, packetCount: packetCount, completion: completion)
                }
            case .failure(let error):
                NSLog("Wispr Lightning: WS receive failed: %@", error.localizedDescription)
                completion(.failure(.connectionFailed))
            }
        }
    }

    func clearEncodingCache() {
        cachedEncoding = nil
    }

    // MARK: - Ascii85 Encoding (matching Python's base64.a85encode)

    private func ascii85Encode(_ data: Data) -> String {
        let byteCount = data.count
        var output = [UInt8]()
        output.reserveCapacity((byteCount / 4 + 1) * 5)

        data.withUnsafeBytes { rawBuffer in
            let bytes = rawBuffer.bindMemory(to: UInt8.self)
            var i = 0
            while i < byteCount {
                var value: UInt32 = 0
                let remaining = min(4, byteCount - i)
                for j in 0..<4 {
                    value = value << 8
                    if j < remaining {
                        value |= UInt32(bytes[i + j])
                    }
                }

                if remaining == 4 && value == 0 {
                    output.append(0x7A) // 'z'
                } else {
                    var encoded: (UInt8, UInt8, UInt8, UInt8, UInt8) = (0, 0, 0, 0, 0)
                    encoded.4 = UInt8(value % 85) + 33; value /= 85
                    encoded.3 = UInt8(value % 85) + 33; value /= 85
                    encoded.2 = UInt8(value % 85) + 33; value /= 85
                    encoded.1 = UInt8(value % 85) + 33; value /= 85
                    encoded.0 = UInt8(value % 85) + 33
                    let outputCount = remaining < 4 ? remaining + 1 : 5
                    output.append(encoded.0)
                    if outputCount > 1 { output.append(encoded.1) }
                    if outputCount > 2 { output.append(encoded.2) }
                    if outputCount > 3 { output.append(encoded.3) }
                    if outputCount > 4 { output.append(encoded.4) }
                }
                i += 4
            }
        }

        return String(bytes: output, encoding: .ascii) ?? ""
    }
}
