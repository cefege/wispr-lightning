import Foundation

/// One audio-input model returned by OpenRouter's `/api/v1/models` listing.
struct OpenRouterAudioModel: Identifiable, Hashable {
    let id: String
    let name: String
    /// USD per 1M *input* tokens (prompt).
    let promptPerMTokens: Double
    /// USD per 1M *output* tokens (completion).
    let completionPerMTokens: Double
    /// USD per 1M *audio* tokens, when the model bills audio separately.
    /// nil when the model doesn't expose a separate audio price.
    let audioPerMTokens: Double?

    /// Human-readable label for the picker: "<name> — $in / $out per 1M".
    var displayLabel: String {
        let inStr = formatPrice(promptPerMTokens)
        let outStr = formatPrice(completionPerMTokens)
        return "\(name) — \(inStr) / \(outStr)"
    }

    private func formatPrice(_ v: Double) -> String {
        if v <= 0 { return "free" }
        return String(format: "$%.2f", v)
    }
}

/// Fetches and caches the OpenRouter model list. Used by Settings →
/// Provider → OpenRouter panel.
enum OpenRouterModels {
    private static let endpoint = URL(string: "https://openrouter.ai/api/v1/models")!

    /// Fetch all audio-input models, sorted by prompt price (cheapest first).
    /// No auth required. Free models (price == 0) bubble to the top.
    static func fetchAudioModels(completion: @escaping (Result<[OpenRouterAudioModel], Error>) -> Void) {
        var request = URLRequest(url: endpoint)
        request.timeoutInterval = 20
        URLSession.shared.dataTask(with: request) { data, response, error in
            if let error {
                DispatchQueue.main.async { completion(.failure(error)) }
                return
            }
            guard let data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let models = json["data"] as? [[String: Any]] else {
                DispatchQueue.main.async {
                    completion(.failure(NSError(domain: "OpenRouterModels", code: 1,
                                                userInfo: [NSLocalizedDescriptionKey: "Malformed model list"])))
                }
                return
            }

            var audio: [OpenRouterAudioModel] = []
            for m in models {
                guard let arch = m["architecture"] as? [String: Any],
                      let modalities = arch["input_modalities"] as? [String],
                      modalities.contains("audio") else { continue }
                guard let id = m["id"] as? String else { continue }
                let name = (m["name"] as? String) ?? id

                let pricing = m["pricing"] as? [String: Any] ?? [:]
                let prompt = parsePrice(pricing["prompt"]) ?? 0
                let completion = parsePrice(pricing["completion"]) ?? 0
                let audioRaw = parsePrice(pricing["audio"])

                audio.append(OpenRouterAudioModel(
                    id: id,
                    name: name,
                    promptPerMTokens: prompt * 1_000_000,
                    completionPerMTokens: completion * 1_000_000,
                    audioPerMTokens: audioRaw.map { $0 * 1_000_000 }
                ))
            }
            audio.sort { (a, b) in
                if a.promptPerMTokens != b.promptPerMTokens {
                    return a.promptPerMTokens < b.promptPerMTokens
                }
                return a.id < b.id
            }
            DispatchQueue.main.async { completion(.success(audio)) }
        }.resume()
    }

    /// OpenRouter prices come as strings like "0.00000025"; some endpoints
    /// return Doubles directly. Treat "-" or "" as nil.
    private static func parsePrice(_ raw: Any?) -> Double? {
        if let d = raw as? Double { return d }
        if let s = raw as? String {
            if s.isEmpty || s == "-" { return nil }
            return Double(s)
        }
        return nil
    }
}
