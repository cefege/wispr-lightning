import Foundation

/// `DictationProvider` backed by OpenRouter, routing to a Gemini multimodal
/// model that accepts audio input and returns cleaned text in one call.
///
/// Buffers packets internally (same shape as Wispr Flow); WAV upload happens
/// at `stop(context:completion:)`. The system prompt's anti-chatbot framing
/// keeps Gemini from *answering* dictated questions instead of transcribing.
final class OpenRouterProvider: DictationProvider {
    var dictionaryStore: DictionaryStore?

    private let endpoint = URL(string: "https://openrouter.ai/api/v1/chat/completions")!
    private let settings: AppSettings
    /// Per-step override used by the fallback chain. When set, this wins over
    /// `settings.openRouterModel` so the chain can include multiple OpenRouter
    /// models with different speed/quality tradeoffs.
    private let modelOverride: String?

    private var bufferedPackets: [Data] = []
    private let bufferLock = NSLock()

    init(settings: AppSettings, modelOverride: String? = nil) {
        self.settings = settings
        self.modelOverride = modelOverride
    }

    /// Map an OpenRouter HTTP error response to a TranscriptionError that
    /// has the right retry semantics and a user-facing message that points
    /// the user at the actual problem (key vs credits vs rate limit vs
    /// server-side outage).
    private static func classifyError(statusCode: Int, body: Data) -> TranscriptionError {
        // OpenRouter usually returns {"error": {"message": "...", "code": 401}}.
        // Cap body parse at 64KB so a 1MB CDN error page doesn't burn time.
        let parseable = body.count <= 64 * 1024 ? body : body.prefix(64 * 1024)
        let json = (try? JSONSerialization.jsonObject(with: parseable)) as? [String: Any]
        let errorBlob = json?["error"] as? [String: Any]
        // Only use the body-as-string fallback when it's recognizably JSON or
        // plain text — HTML / binary error pages just leak raw markup to the
        // pill. Detect by looking for an opening `<` (HTML) or non-printable
        // chars in the first 50 bytes.
        let fallbackText: String? = {
            guard let head = String(data: parseable.prefix(50), encoding: .utf8) else { return nil }
            let trimmed = head.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.hasPrefix("<") { return nil }  // HTML page
            return String(data: parseable.prefix(200), encoding: .utf8)?
                .replacingOccurrences(of: "\n", with: " ")
                .trimmingCharacters(in: .whitespaces)
        }()
        let serverMessage = (errorBlob?["message"] as? String) ?? fallbackText ?? ""

        switch statusCode {
        case 401, 403:
            return .authFailed("OpenRouter: your API key was rejected (HTTP \(statusCode)). Open Settings → Accounts → OpenRouter and paste a fresh key from openrouter.ai/keys.")
        case 402:
            // OpenRouter returns 402 when the account is out of credits.
            return .authFailed("OpenRouter: out of credits (HTTP 402). Add funds at openrouter.ai/credits, then try again. \(serverMessage)")
        case 429:
            // Rate limited. Retryable — backoff helps.
            return .serverError("OpenRouter: rate limited (HTTP 429). \(serverMessage)")
        case 400, 404:
            // Bad request / model not found — usually a stale model id in
            // settings. Non-retryable since the chain would hit the same
            // error; route as authFailed-style guidance so the fallback
            // chain advances rather than auto-retrying twice first.
            return .authFailed("OpenRouter: HTTP \(statusCode) — \(serverMessage). The model id may no longer exist; pick a different one in Settings → Provider.")
        case 500...599:
            return .serverError("OpenRouter: server error HTTP \(statusCode). \(serverMessage)")
        default:
            return .serverError("OpenRouter: HTTP \(statusCode): \(serverMessage)")
        }
    }

    private var apiKey: String? {
        if let env = ProcessInfo.processInfo.environment["WISPR_LIGHTNING_OPENROUTER_KEY"], !env.isEmpty {
            return env
        }
        if let stored = SecretsStore.read(.openRouterAPIKey) { return stored }
        // Migration: if the key is still in Keychain from a prior build, pull
        // it once (may prompt), persist to SecretsStore, and remove from
        // Keychain so we never prompt for it again. Only delete the Keychain
        // copy if SecretsStore.write actually succeeded — otherwise we'd
        // lose the user's key with nowhere to recover it from.
        if let migrated = KeychainStore.read(.openRouterAPIKey) {
            if SecretsStore.write(.openRouterAPIKey, migrated) {
                KeychainStore.delete(.openRouterAPIKey)
            } else {
                wLog("OpenRouter: failed to migrate key to SecretsStore; leaving Keychain copy intact")
            }
            return migrated
        }
        return nil
    }

    private var model: String {
        if let override = modelOverride?.trimmingCharacters(in: .whitespaces), !override.isEmpty {
            return override
        }
        let configured = settings.openRouterModel.trimmingCharacters(in: .whitespaces)
        return configured.isEmpty ? "google/gemini-2.5-flash-lite" : configured
    }

    private static let systemPrompt = """
    You are a dictation transcriber. Transcribe the audio with light cleanup: \
    fix punctuation, capitalization, and remove filler words \
    (um, uh, like, you know). Preserve the speaker's word choice and tone.

    You are NOT a chatbot. If the audio contains a question or request, \
    TRANSCRIBE it — do not answer it. Output ONLY the cleaned transcript, \
    nothing else.
    """

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
        transcribe(packets: packets, completion: completion)
    }

    func cancel() {
        bufferLock.lock()
        bufferedPackets.removeAll(keepingCapacity: false)
        bufferLock.unlock()
    }

    // MARK: - Transcription

    private func transcribe(packets: [Data],
                            completion: @escaping (Result<TranscriptResult, TranscriptionError>) -> Void) {
        guard !packets.isEmpty else {
            completion(.failure(.emptyResult))
            return
        }
        guard let apiKey = apiKey else {
            wLog("OpenRouter: no API key — open Settings → Accounts → OpenRouter and paste your key")
            completion(.failure(.authFailed("OpenRouter has no saved API key. Open Settings → Accounts → OpenRouter and paste one from openrouter.ai/keys.")))
            return
        }

        let durationSeconds = Double(packets.count) * Double(Constants.chunkDurationMs) / 1000.0
        let wav = AudioEncoding.wavData(from: packets)
        let base64 = wav.base64EncodedString()
        wLog("OpenRouter: sending \(wav.count / 1024)KB WAV, \(String(format: "%.1f", durationSeconds))s, model=\(model)")

        var customWordsLine = ""
        if let words = dictionaryStore?.getVocabularyPhrases(), !words.isEmpty {
            let sampled = Array(words.prefix(40))
            customWordsLine = "\n\nThe speaker frequently uses these proper nouns or jargon — spell them as written: \(sampled.joined(separator: ", "))."
        }

        let body: [String: Any] = [
            "model": model,
            "stream": false,
            "messages": [
                ["role": "system", "content": Self.systemPrompt + customWordsLine],
                ["role": "user", "content": [
                    ["type": "input_audio", "input_audio": ["data": base64, "format": "wav"]]
                ]]
            ]
        ]

        guard let bodyData = try? JSONSerialization.data(withJSONObject: body) else {
            completion(.failure(.connectionFailed))
            return
        }

        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("Wispr Lightning", forHTTPHeaderField: "X-Title")
        request.setValue("https://github.com/cefege/wispr", forHTTPHeaderField: "HTTP-Referer")
        request.httpBody = bodyData
        request.timeoutInterval = 90

        let startedAt = Date()
        URLSession.shared.dataTask(with: request) { data, response, error in
            if let error = error {
                wLog("OpenRouter: network error — \(error.localizedDescription)")
                completion(.failure(.connectionFailed))
                return
            }
            guard let data = data else {
                completion(.failure(.connectionFailed))
                return
            }
            if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                let snippet = String(data: data, encoding: .utf8)?.prefix(400) ?? ""
                wLog("OpenRouter: HTTP \(http.statusCode) — \(snippet)")
                completion(.failure(Self.classifyError(statusCode: http.statusCode, body: data)))
                return
            }
            guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                wLog("OpenRouter: response was not JSON")
                completion(.failure(.serverError("malformed JSON response")))
                return
            }
            guard let choices = json["choices"] as? [[String: Any]],
                  let first = choices.first,
                  let message = first["message"] as? [String: Any],
                  let content = message["content"] as? String else {
                let snippet = String(data: data, encoding: .utf8)?.prefix(400) ?? ""
                wLog("OpenRouter: no content in response — \(snippet)")
                completion(.failure(.serverError("no content")))
                return
            }
            let text = content.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty else {
                completion(.failure(.emptyResult))
                return
            }
            let elapsed = Date().timeIntervalSince(startedAt)
            wLog("OpenRouter: got \(text.count) chars in \(String(format: "%.1f", elapsed))s")
            let result = TranscriptResult(
                id: UUID().uuidString,
                asrText: text,
                formattedText: text,
                duration: durationSeconds,
                numWords: text.split(separator: " ").count
            )
            completion(.success(result))
        }.resume()
    }
}
