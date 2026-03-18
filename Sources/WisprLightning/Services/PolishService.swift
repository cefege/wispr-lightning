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
        guard let url = URL(string: "\(Constants.apiURL)/llm/polish_text") else {
            completion(.failure(.connectionFailed))
            return
        }

        let polishUUID = UUID().uuidString
        let startTime = Date()
        let instructionString = instructions.joined(separator: ". ")

        // Match Wispr Flow's request format exactly — instructions is a dict {text: bool}
        let instructionsDict = instructions.reduce(into: [String: Bool]()) { $0[$1] = true }
        let body: [String: Any] = [
            "selected_text": text,
            "instructions": instructionsDict,
            "provider_config": NSNull(),
            "writing_samples": NSNull(),
            "custom_prompt": NSNull()
        ]

        guard let bodyData = try? JSONSerialization.data(withJSONObject: body) else {
            completion(.failure(.connectionFailed))
            return
        }

        wLogVerbose("Polish request body: \(String(data: bodyData, encoding: .utf8) ?? "")")

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        // Wispr Flow sends the raw token without "Bearer" prefix
        request.setValue(session.accessToken ?? "", forHTTPHeaderField: "Authorization")
        request.setValue("no-cache, no-store, must-revalidate", forHTTPHeaderField: "Cache-Control")
        request.httpBody = bodyData

        URLSession.shared.dataTask(with: request) { data, response, error in
            if let error = error {
                NSLog("Wispr Lightning: Polish request failed: %@", error.localizedDescription)
                completion(.failure(.connectionFailed))
                return
            }

            guard let data = data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                NSLog("Wispr Lightning: Polish response parse failed")
                completion(.failure(.connectionFailed))
                return
            }

            wLogVerbose("Polish response: \(String(data: data, encoding: .utf8) ?? "")")

            guard let polishedText = json["polished_text"] as? String, !polishedText.isEmpty else {
                let status = json["status"] as? String ?? "unknown"
                NSLog("Wispr Lightning: Polish failed with status: %@", status)
                completion(.failure(.serverError(status)))
                return
            }

            let processingTime = Date().timeIntervalSince(startTime)
            let result = PolishResult(
                id: polishUUID,
                initialText: text,
                polishedText: polishedText,
                initialWordCount: text.split(separator: " ").count,
                polishedWordCount: polishedText.split(separator: " ").count,
                processingTime: processingTime,
                instruction: instructionString
            )
            completion(.success(result))
        }.resume()
    }
}
