import Foundation

struct DictionaryEntry: Identifiable, Hashable {
    let id: String
    let phrase: String
    let replacement: String?
    let isSnippet: Bool
    let manualEntry: Bool
    let source: String?
    let frequencyUsed: Int
    let createdAt: Date
    let modifiedAt: Date

    static func == (lhs: DictionaryEntry, rhs: DictionaryEntry) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
}
