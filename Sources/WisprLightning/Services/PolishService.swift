import Foundation

struct PolishResult {
    let id: String
    let initialText: String
    let polishedText: String
    let initialWordCount: Int
    let polishedWordCount: Int
    let processingTime: Double
    let instruction: String
}

class PolishService {
    private let session: Session
    private let settings: AppSettings

    init(session: Session, settings: AppSettings) {
        self.session = session
        self.settings = settings
    }

    func polish(text: String, instructions: [String], completion: @escaping (Result<PolishResult, TranscriptionError>) -> Void) {
        guard !text.isEmpty else {
            completion(.failure(.emptyResult))
            return
        }

        guard session.isValid else {
            session.refresh { [weak self] success in
                guard success, let self = self else {
                    completion(.failure(.authFailed))
                    return
                }
                self.performPolish(text: text, instructions: instructions, completion: completion)
            }
            return
        }

        performPolish(text: text, instructions: instructions, completion: completion)
    }

    private func performPolish(text: String, instructions: [String], completion: @escaping (Result<PolishResult, TranscriptionError>) -> Void) {
        guard let url = URL(string: Constants.wsURL) else {
            completion(.failure(.connectionFailed))
            return
        }

        var request = URLRequest(url: url)
        request.setValue("json", forHTTPHeaderField: "Encoding")
        let wsTask = URLSession.shared.webSocketTask(with: request)
        wsTask.resume()

        let polishUUID = UUID().uuidString
        let startTime = Date()
        let instructionString = instructions.joined(separator: ". ")

        let authMsg: [String: Any] = [
            "type": "auth",
            "access_token": session.accessToken ?? "",
            "app": "other",
            "context": [
                "app": ["name": "", "bundle_id": "", "type": "other", "url": ""],
                "ax_context": [] as [Any],
                "ocr_context": [] as [Any],
                "dictionary_context": [] as [Any],
                "dictionary_replacements": [:] as [String: Any],
                "dictionary_snippets": [:] as [String: Any],
                "user_first_name": session.userFirstName ?? "",
                "user_last_name": session.userLastName ?? "",
                "textbox_contents": [:] as [String: Any],
                "content_text": text,
                "variable_names": [] as [Any],
                "file_names": [] as [Any]
            ] as [String: Any],
            "personalization_style_settings": [:] as [String: String],
            "language": settings.languages,
            "metadata": [
                "session_id": session.sessionId,
                "environment": "PRODUCTION",
                "client_platform": "darwin",
                "client_version": Constants.clientVersion,
                "transcript_entity_uuid": polishUUID
            ] as [String: Any],
            "pipeline": ["polish"],
            "polish_instructions": instructionString,
            "polish_text": text,
            "cleanup_level": "none",
            "command_mode": false,
            "debug_mode": false,
            "use_staging_baseten": false,
            "prefix_is_written": false,
            "hyperlink_on": false
        ]

        guard let authData = try? JSONSerialization.data(withJSONObject: authMsg),
              let authString = String(data: authData, encoding: .utf8) else {
            wsTask.cancel(with: .internalServerError, reason: nil)
            completion(.failure(.connectionFailed))
            return
        }

        wsTask.send(.string(authString)) { error in
            if let error = error {
                NSLog("Wispr Lightning: Polish WS auth send failed: %@", error.localizedDescription)
                completion(.failure(.connectionFailed))
                return
            }
        }

        wsTask.receive { [weak self] result in
            switch result {
            case .success(let message):
                if case .string(let responseText) = message,
                   let data = responseText.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   json["status"] as? String == "auth" {
                    // Send commit to trigger polish processing
                    let commitMsg: [String: Any] = [
                        "type": "commit",
                        "total_packets": 0
                    ]
                    guard let commitData = try? JSONSerialization.data(withJSONObject: commitMsg),
                          let commitString = String(data: commitData, encoding: .utf8) else {
                        completion(.failure(.connectionFailed))
                        return
                    }
                    wsTask.send(.string(commitString)) { error in
                        if let error = error {
                            NSLog("Wispr Lightning: Polish commit failed: %@", error.localizedDescription)
                            completion(.failure(.connectionFailed))
                            return
                        }
                        self?.receivePolishResult(wsTask: wsTask, polishUUID: polishUUID, originalText: text, startTime: startTime, instruction: instructionString, completion: completion)
                    }
                } else {
                    wsTask.cancel(with: .internalServerError, reason: nil)
                    completion(.failure(.authFailed))
                }
            case .failure:
                completion(.failure(.connectionFailed))
            }
        }
    }

    private func receivePolishResult(wsTask: URLSessionWebSocketTask, polishUUID: String, originalText: String, startTime: Date, instruction: String, completion: @escaping (Result<PolishResult, TranscriptionError>) -> Void) {
        var completed = false
        let completionLock = NSLock()

        let safeComplete: (Result<PolishResult, TranscriptionError>) -> Void = { result in
            completionLock.lock()
            guard !completed else {
                completionLock.unlock()
                return
            }
            completed = true
            completionLock.unlock()
            completion(result)
        }

        let timeoutWork = DispatchWorkItem {
            wsTask.cancel(with: .abnormalClosure, reason: nil)
            safeComplete(.failure(.timeout))
        }
        DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 15, execute: timeoutWork)

        receiveLoop(wsTask: wsTask, polishUUID: polishUUID, originalText: originalText, startTime: startTime, instruction: instruction) { result in
            timeoutWork.cancel()
            safeComplete(result)
        }
    }

    private func receiveLoop(wsTask: URLSessionWebSocketTask, polishUUID: String, originalText: String, startTime: Date, instruction: String, completion: @escaping (Result<PolishResult, TranscriptionError>) -> Void) {
        wsTask.receive { [weak self] result in
            switch result {
            case .success(let message):
                if case .string(let responseText) = message,
                   let data = responseText.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {

                    let status = json["status"] as? String
                    if status == "text" {
                        let body = json["body"] as? [String: Any] ?? [:]
                        let polishedText = body["llm_text"] as? String ?? body["asr_text"] as? String ?? ""
                        let isFinal = json["final"] as? Bool ?? false

                        if isFinal {
                            let processingTime = Date().timeIntervalSince(startTime)
                            let polishResult = PolishResult(
                                id: polishUUID,
                                initialText: originalText,
                                polishedText: polishedText,
                                initialWordCount: originalText.split(separator: " ").count,
                                polishedWordCount: polishedText.split(separator: " ").count,
                                processingTime: processingTime,
                                instruction: instruction
                            )
                            wsTask.cancel(with: .normalClosure, reason: nil)
                            if polishedText.isEmpty {
                                completion(.failure(.emptyResult))
                            } else {
                                completion(.success(polishResult))
                            }
                            return
                        }
                    } else if status == "error" {
                        let errorDetail = json["error"] as? String ?? "unknown"
                        wsTask.cancel(with: .internalServerError, reason: nil)
                        completion(.failure(.serverError(errorDetail)))
                        return
                    }

                    self?.receiveLoop(wsTask: wsTask, polishUUID: polishUUID, originalText: originalText, startTime: startTime, instruction: instruction, completion: completion)
                }
            case .failure(let error):
                NSLog("Wispr Lightning: Polish receive failed: %@", error.localizedDescription)
                completion(.failure(.connectionFailed))
            }
        }
    }
}
