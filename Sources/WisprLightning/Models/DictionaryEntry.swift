import Foundation

struct DictionaryEntry: Identifiable {
    let id: String
    let phrase: String
    let replacement: String?
    let isSnippet: Bool
    let manualEntry: Bool
    let source: String?
    let frequencyUsed: Int
    let createdAt: Date
    let modifiedAt: Date
}
