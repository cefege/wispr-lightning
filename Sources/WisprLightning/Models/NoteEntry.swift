import Foundation

struct NoteEntry: Identifiable {
    let id: String
    var title: String
    var contentPreview: String
    var content: String
    let createdAt: Date
    var modifiedAt: Date
}
