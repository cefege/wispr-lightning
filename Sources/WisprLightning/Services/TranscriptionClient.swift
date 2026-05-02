import Foundation

class TranscriptionClient {
    private let session: Session
    private let settings: AppSettings
    var dictionaryStore: DictionaryStore?
    private var prewarmedTask: URLSessionWebSocketTask?
    private let prewarmLock = NSLock()
    /// Max packets per WebSocket append message (~20 seconds of audio, ~800KB encoded)
    private static let chunkSize = 500
    private var cachedEncoding: (packetCount: Int, prepared: PreparedAudio)?
    private let encodingQueue = DispatchQueue(label: "com.wisprlightning.encode", qos: .userInitiated)

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

    private func createWebSocketTask() -> URLSessionWebSocketTask? {
        guard let url = URL(string: Constants.wsURL) else { return nil }
        var request = URLRequest(url: url)
        request.setValue("json", forHTTPHeaderField: "Encoding")
        let task = URLSession.shared.webSocketTask(with: request)
        task.maximumMessageSize = 10 * 1024 * 1024 // 10MB receive buffer
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

        // Use prewarmed connection if available and still connected (TCP+TLS already done)
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
                prewarmed.cancel(with: .normalClosure, reason: nil)
            }
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
                            self.sendPreparedAudio(wsTask: wsTask, prepared: preparedAudio, packetCount: packets.count, transcriptUUID: transcriptUUID, completion: safeComplete)
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

        wsTask.send(.string(commitString)) { error in
            if let error = error {
                NSLog("Wispr Lightning: WS commit send failed: %@", error.localizedDescription)
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
        wsTask.send(.string(appendString)) { [self] error in
            if let error = error {
                NSLog("Wispr Lightning: WS chunk send failed: %@", error.localizedDescription)
                completion(.failure(.connectionFailed))
                return
            }

            if isFinal {
                self.sendCommitAndReceive(wsTask: wsTask, totalPackets: totalPackets, transcriptUUID: transcriptUUID, completion: completion)
            } else {
                // Send next chunk
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

        // Start timeout deadline — scales with recording duration
        let timeout = Self.responseTimeout(for: packetCount)
        let timeoutWork = DispatchWorkItem {
            NSLog("Wispr Lightning: WebSocket response timed out after %.0fs", timeout)
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
