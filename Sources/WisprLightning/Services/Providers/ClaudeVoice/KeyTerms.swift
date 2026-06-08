import Foundation
import NaturalLanguage

/// Vocabulary boost extractor for the Claude Voice provider.
///
/// Wispr Flow's transcription pipeline takes a full OCR context blob and feeds
/// it to an LLM formatter. The Claude Code STT endpoint can only consume
/// Deepgram's `keyterms` vocabulary boost (a list of strings in the WS URL),
/// so we distil OCR / dictionary lines down to high-signal nouns, drop UI
/// noise / stopwords / short tokens / numerics, dedupe, and return the
/// top-N by frequency.
enum ClaudeVoiceKeyTerms {
    static let stopwords: Set<String> = [
        "the", "and", "for", "with", "from", "this", "that", "these", "those",
        "have", "has", "had", "are", "was", "were", "been", "being", "but", "not",
        "you", "your", "they", "their", "them", "him", "her", "his",
        "what", "when", "where", "which", "who", "why", "how",
        "can", "will", "would", "could", "should", "may", "might", "must",
        "ok", "okay", "yes", "no", "cancel", "settings", "edit", "view", "file",
        "open", "close", "save", "delete", "new", "home", "back", "next",
        "search", "send", "reply", "type", "click", "tap",
    ]

    static func extract(from lines: [String], limit: Int = 20) -> [String] {
        guard !lines.isEmpty else { return [] }

        var counts: [String: Int] = [:]
        let tagger = NLTagger(tagSchemes: [.lexicalClass])
        let options: NLTagger.Options = [.omitPunctuation, .omitWhitespace, .joinNames]

        for line in lines {
            tagger.string = line
            tagger.enumerateTags(in: line.startIndex..<line.endIndex, unit: .word, scheme: .lexicalClass, options: options) { tag, range in
                guard let tag = tag else { return true }
                guard tag == .noun || tag == .placeName || tag == .personalName || tag == .organizationName else {
                    return true
                }
                let token = String(line[range])
                guard let cleaned = clean(token) else { return true }
                counts[cleaned, default: 0] += 1
                return true
            }
        }

        let ranked = counts
            .sorted { (l, r) in
                if l.value != r.value { return l.value > r.value }
                return l.key < r.key
            }
            .prefix(limit)
            .map { $0.key }
        return Array(ranked)
    }

    private static func clean(_ raw: String) -> String? {
        let trimmed = raw.trimmingCharacters(in: .punctuationCharacters)
            .trimmingCharacters(in: .whitespaces)
        guard trimmed.count >= 4 else { return nil }
        guard !isNumeric(trimmed) else { return nil }
        let lower = trimmed.lowercased()
        guard !stopwords.contains(lower) else { return nil }
        return trimmed
    }

    private static func isNumeric(_ s: String) -> Bool {
        return s.unicodeScalars.allSatisfy { CharacterSet.decimalDigits.contains($0) || $0 == "." || $0 == "," }
    }
}
