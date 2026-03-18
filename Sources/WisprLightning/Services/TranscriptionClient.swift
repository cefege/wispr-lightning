import Foundation

class TranscriptionClient {
    private let session: Session
    private let settings: AppSettings
    var dictionaryStore: DictionaryStore?
    private var prewarmedTask: URLSessionWebSocketTask?
    private let prewarmLock = NSLock()
    private static let responseTimeoutSeconds: Double = 10
    private var cachedEncoding: (packetCount: Int, encoded: String)?
    private let encodingQueue = DispatchQueue(label: "com.wisprlightning.encode", qos: .userInitiated)

    init(session: Session, settings: AppSettings) {
        self.session = session
        self.settings = settings
    }

    private func createWebSocketTask() -> URLSessionWebSocketTask? {
        guard let url = URL(string: Constants.wsURL) else { return nil }
        var request = URLRequest(url: url)
        request.setValue("json", forHTTPHeaderField: "Encoding")
        let task = URLSession.shared.webSocketTask(with: request)
        task.resume()
        return task
    }

    /// Start TCP+TLS handshake early so it's ready when audio finishes
    func prewarmConnection() {
        guard let task = createWebSocketTask() else { return }
        prewarmLock.lock()
        prewarmedTask = task
        prewarmLock.unlock()

        // Proactively refresh token if expired, so it's ready when transcription starts
        if !session.isValid {
            session.refresh { success in
                if !success {
                    NSLog("Wispr Lightning: Proactive token refresh failed")
                }
            }
        }
    }

    /// Cancel a prewarmed connection that won't be used (e.g. recording too short)
    func cancelPrewarmedConnection() {
        prewarmLock.lock()
        let task = prewarmedTask
        prewarmedTask = nil
        prewarmLock.unlock()
        task?.cancel(with: .normalClosure, reason: nil)
    }

    func transcribe(packets: [Data], appInfo: [String: String], ocrContext: [String] = [], axContext: [String] = [], completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        guard !packets.isEmpty else {
            completion(.failure(.emptyResult))
            return
        }

        // Ensure valid token
        guard session.isValid else {
            NSLog("Wispr Lightning: Token expired, refreshing...")
            session.refresh { [weak self] success in
                guard success, let self = self else {
                    NSLog("Wispr Lightning: Cannot transcribe — auth failed")
                    completion(.failure(.authFailed))
                    return
                }
                self.performTranscription(packets: packets, appInfo: appInfo, ocrContext: ocrContext, axContext: axContext, completion: completion)
            }
            return
        }

        performTranscription(packets: packets, appInfo: appInfo, ocrContext: ocrContext, axContext: axContext, completion: completion)
    }

    private func performTranscription(packets: [Data], appInfo: [String: String], ocrContext: [String] = [], axContext: [String] = [], completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        // Guard against double-completion — send and receive error callbacks can both fire
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

        // Use prewarmed connection if available (TCP+TLS already done)
        let wsTask: URLSessionWebSocketTask
        prewarmLock.lock()
        let prewarmed = prewarmedTask
        prewarmedTask = nil
        prewarmLock.unlock()
        if let prewarmed = prewarmed {
            wsTask = prewarmed
        } else {
            guard let newTask = createWebSocketTask() else {
                safeComplete(.failure(.connectionFailed))
                return
            }
            wsTask = newTask
        }

        let transcriptUUID = UUID().uuidString
        let appType = (appInfo["type"] ?? "other").lowercased()

        // 1. Send auth message
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
                "ax_context": axContext,
                "ocr_context": ocrContext,
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
            "prefix_is_written": !axContext.isEmpty,
            "hyperlink_on": settings.hyperlinkOn
        ]

        guard let authData = try? JSONSerialization.data(withJSONObject: authMsg),
              let authString = String(data: authData, encoding: .utf8) else {
            wsTask.cancel(with: .internalServerError, reason: nil)
            safeComplete(.failure(.connectionFailed))
            return
        }

        // Start encoding audio (reuse cache on retry, or encode in parallel with auth)
        var preparedAppendString: String?
        var encodeGroup: DispatchGroup?

        if let cached = cachedEncoding, cached.packetCount == packets.count {
            preparedAppendString = cached.encoded
        } else {
            let group = DispatchGroup()
            group.enter()
            encodingQueue.async {
                preparedAppendString = self.prepareAudioMessage(packets: packets)
                if let encoded = preparedAppendString {
                    self.cachedEncoding = (packetCount: packets.count, encoded: encoded)
                }
                group.leave()
            }
            encodeGroup = group
        }

        wLogVerbose("WS sending auth — token: \(String((session.accessToken ?? "").prefix(8)))..., app: \(appType), pipeline: \(pipeline.joined(separator: ","))")

        // Send auth message
        wsTask.send(.string(authString)) { error in
            if let error = error {
                NSLog("Wispr Lightning: WS auth send failed: %@", error.localizedDescription)
                safeComplete(.failure(.connectionFailed))
                return
            }
        }

        // Receive auth response, then send audio
        wsTask.receive { [weak self] result in
            guard let self = self else { return }
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
                            self.sendPreparedAudio(wsTask: wsTask, appendString: preparedAppendString, packetCount: packets.count, transcriptUUID: transcriptUUID, completion: safeComplete)
                        }
                        if let group = encodeGroup {
                            group.notify(queue: self.encodingQueue, execute: sendAudio)
                        } else {
                            sendAudio()
                        }
                    } else {
                        wLog("WebSocket auth failed — unexpected response")
                        wsTask.cancel(with: .internalServerError, reason: nil)
                        safeComplete(.failure(.authFailed))
                    }
                } else {
                    wLog("WebSocket auth failed — non-string message received")
                    wsTask.cancel(with: .internalServerError, reason: nil)
                    safeComplete(.failure(.authFailed))
                }
            case .failure(let error):
                wLog("WS receive failed: \(error.localizedDescription)")
                safeComplete(.failure(.connectionFailed))
            }
        }
    }

    private func prepareAudioMessage(packets: [Data]) -> String? {
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

        let appendMsg: [String: Any] = [
            "type": "append",
            "audio_packets": [
                "packets": encodedPackets,
                "volumes": volumes,
                "packet_duration": Double(Constants.chunkDurationMs) / 1000.0,
                "audio_encoding": "wav",
                "byte_encoding": "ascii85"
            ] as [String: Any],
            "position": 0,
            "final": true
        ]

        guard let appendData = try? JSONSerialization.data(withJSONObject: appendMsg),
              let appendString = String(data: appendData, encoding: .utf8) else {
            return nil
        }
        return appendString
    }

    private func sendPreparedAudio(wsTask: URLSessionWebSocketTask, appendString: String?, packetCount: Int, transcriptUUID: String, completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        guard let appendString = appendString else {
            wsTask.cancel(with: .internalServerError, reason: nil)
            completion(.failure(.connectionFailed))
            return
        }

        wLogVerbose("WS sending append — \(packetCount) packets")
        wsTask.send(.string(appendString)) { error in
            if let error = error {
                NSLog("Wispr Lightning: WS append send failed: %@", error.localizedDescription)
                completion(.failure(.connectionFailed))
                return
            }

            // Send commit
            let commitMsg: [String: Any] = [
                "type": "commit",
                "total_packets": packetCount
            ]
            guard let commitData = try? JSONSerialization.data(withJSONObject: commitMsg),
                  let commitString = String(data: commitData, encoding: .utf8) else {
                completion(.failure(.connectionFailed))
                return
            }

            wsTask.send(.string(commitString)) { error in
                if let error = error {
                    NSLog("Wispr Lightning: WS commit send failed: %@", error.localizedDescription)
                    completion(.failure(.connectionFailed))
                    return
                }

                NSLog("Wispr Lightning: Audio sent — %d packets, waiting for transcription...", packetCount)
                self.receiveResultWithTimeout(wsTask: wsTask, transcriptUUID: transcriptUUID, packetCount: packetCount, completion: completion)
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

        // Start timeout deadline
        let timeoutWork = DispatchWorkItem {
            NSLog("Wispr Lightning: WebSocket response timed out after %.0fs", Self.responseTimeoutSeconds)
            wsTask.cancel(with: .abnormalClosure, reason: nil)
            safeComplete(.failure(.timeout))
        }
        DispatchQueue.global(qos: .userInitiated).asyncAfter(
            deadline: .now() + Self.responseTimeoutSeconds,
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
                        wsTask.cancel(with: .internalServerError, reason: nil)
                        completion(.failure(.serverError(errorDetail)))
                        return
                    } else if status == "info" {
                        NSLog("Wispr Lightning: Server info: %@", json["message"] as? String ?? "")
                    }

                    // Continue receiving
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
        // Pre-allocate output buffer: each 4-byte group becomes at most 5 bytes
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
