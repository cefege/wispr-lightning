import Foundation

class TranscriptionClient {
    private let session: Session

    init(session: Session) {
        self.session = session
    }

    func transcribe(packets: [Data], appInfo: [String: String], ocrContext: [String] = [], completion: @escaping (TranscriptResult?) -> Void) {
        guard !packets.isEmpty else {
            completion(nil)
            return
        }

        // Ensure valid token
        guard session.isValid else {
            NSLog("Wispr Lightning: Token expired, refreshing...")
            session.refresh { [weak self] success in
                guard success, let self = self else {
                    NSLog("Wispr Lightning: Cannot transcribe — auth failed")
                    completion(nil)
                    return
                }
                self.performTranscription(packets: packets, appInfo: appInfo, ocrContext: ocrContext, completion: completion)
            }
            return
        }

        performTranscription(packets: packets, appInfo: appInfo, ocrContext: ocrContext, completion: completion)
    }

    private func performTranscription(packets: [Data], appInfo: [String: String], ocrContext: [String] = [], completion: @escaping (TranscriptResult?) -> Void) {
        guard let url = URL(string: Constants.wsURL) else {
            completion(nil)
            return
        }

        var request = URLRequest(url: url)
        request.setValue("json", forHTTPHeaderField: "Encoding")

        let wsTask = URLSession.shared.webSocketTask(with: request)
        wsTask.resume()

        let transcriptUUID = UUID().uuidString
        let appType = (appInfo["type"] ?? "other").lowercased()

        // 1. Send auth message
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
                "ax_context": [] as [Any],
                "ocr_context": ocrContext,
                "dictionary_context": [] as [Any],
                "dictionary_replacements": [:] as [String: Any],
                "dictionary_snippets": [:] as [String: Any],
                "user_first_name": session.userFirstName ?? "",
                "user_last_name": session.userLastName ?? "",
                "textbox_contents": [:] as [String: Any],
                "content_text": "",
                "variable_names": [] as [Any],
                "file_names": [] as [Any]
            ] as [String: Any],
            "personalization_style_settings": [:] as [String: Any],
            "language": ["en"],
            "metadata": [
                "session_id": session.sessionId,
                "environment": "PRODUCTION",
                "client_platform": "darwin",
                "client_version": Constants.clientVersion,
                "transcript_entity_uuid": transcriptUUID
            ] as [String: Any],
            "pipeline": ["transcribe", "format"],
            "debug_mode": false,
            "use_staging_baseten": false,
            "prefix_is_written": false,
            "hyperlink_on": false
        ]

        guard let authData = try? JSONSerialization.data(withJSONObject: authMsg),
              let authString = String(data: authData, encoding: .utf8) else {
            wsTask.cancel(with: .internalServerError, reason: nil)
            completion(nil)
            return
        }

        wsTask.send(.string(authString)) { error in
            if let error = error {
                NSLog("Wispr Lightning: WS auth send failed: %@", error.localizedDescription)
                completion(nil)
                return
            }
        }

        // 2. Receive auth response
        wsTask.receive { [weak self] result in
            guard let self = self else { return }

            switch result {
            case .success(let message):
                if case .string(let text) = message,
                   let data = text.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   json["status"] as? String == "auth" {
                    NSLog("Wispr Lightning: WebSocket authenticated")
                    self.sendAudio(wsTask: wsTask, packets: packets, transcriptUUID: transcriptUUID, completion: completion)
                } else {
                    NSLog("Wispr Lightning: WebSocket auth failed")
                    wsTask.cancel(with: .internalServerError, reason: nil)
                    completion(nil)
                }
            case .failure(let error):
                NSLog("Wispr Lightning: WS receive failed: %@", error.localizedDescription)
                completion(nil)
            }
        }
    }

    private func sendAudio(wsTask: URLSessionWebSocketTask, packets: [Data], transcriptUUID: String, completion: @escaping (TranscriptResult?) -> Void) {
        // Encode packets as ascii85 and compute volumes
        var encodedPackets: [String] = []
        var volumes: [Double] = []

        for packet in packets {
            encodedPackets.append(ascii85Encode(packet))

            // Calculate volume (RMS of Int16 samples)
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
            wsTask.cancel(with: .internalServerError, reason: nil)
            completion(nil)
            return
        }

        wsTask.send(.string(appendString)) { error in
            if let error = error {
                NSLog("Wispr Lightning: WS append send failed: %@", error.localizedDescription)
                completion(nil)
                return
            }

            // Send commit
            let commitMsg: [String: Any] = [
                "type": "commit",
                "total_packets": packets.count
            ]
            guard let commitData = try? JSONSerialization.data(withJSONObject: commitMsg),
                  let commitString = String(data: commitData, encoding: .utf8) else {
                completion(nil)
                return
            }

            wsTask.send(.string(commitString)) { error in
                if let error = error {
                    NSLog("Wispr Lightning: WS commit send failed: %@", error.localizedDescription)
                    completion(nil)
                    return
                }

                NSLog("Wispr Lightning: Audio sent — %d packets, waiting for transcription...", packets.count)
                self.receiveResult(wsTask: wsTask, transcriptUUID: transcriptUUID, packetCount: packets.count, completion: completion)
            }
        }
    }

    private func receiveResult(wsTask: URLSessionWebSocketTask, transcriptUUID: String, packetCount: Int, completion: @escaping (TranscriptResult?) -> Void) {
        wsTask.receive { [weak self] result in
            switch result {
            case .success(let message):
                if case .string(let text) = message,
                   let data = text.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {

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
                            completion(transcriptResult)
                            return
                        }
                    } else if status == "error" {
                        NSLog("Wispr Lightning: Server error: %@", json["error"] as? String ?? "unknown")
                        wsTask.cancel(with: .internalServerError, reason: nil)
                        completion(nil)
                        return
                    } else if status == "info" {
                        NSLog("Wispr Lightning: Server info: %@", json["message"] as? String ?? "")
                    }

                    // Continue receiving
                    self?.receiveResult(wsTask: wsTask, transcriptUUID: transcriptUUID, packetCount: packetCount, completion: completion)
                }
            case .failure(let error):
                NSLog("Wispr Lightning: WS receive failed: %@", error.localizedDescription)
                completion(nil)
            }
        }
    }

    // MARK: - Ascii85 Encoding (matching Python's base64.a85encode)

    private func ascii85Encode(_ data: Data) -> String {
        var result = ""
        let bytes = [UInt8](data)
        var i = 0

        while i < bytes.count {
            // Pack up to 4 bytes into a 32-bit value
            var value: UInt32 = 0
            let remaining = min(4, bytes.count - i)
            for j in 0..<4 {
                value = value << 8
                if j < remaining {
                    value |= UInt32(bytes[i + j])
                }
            }

            if remaining == 4 && value == 0 {
                result.append("z")
            } else {
                // Encode as 5 ascii85 characters
                var encoded = [Character](repeating: "!", count: 5)
                for j in stride(from: 4, through: 0, by: -1) {
                    encoded[j] = Character(UnicodeScalar(Int(value % 85) + 33)!)
                    value /= 85
                }
                // For partial blocks, only output remaining+1 characters
                let outputCount = remaining < 4 ? remaining + 1 : 5
                result.append(contentsOf: encoded[0..<outputCount])
            }

            i += 4
        }

        return result
    }
}
