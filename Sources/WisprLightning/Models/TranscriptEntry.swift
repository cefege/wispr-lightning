import Foundation

struct TranscriptEntry: Identifiable {
    let id: String
    let asrText: String?
    let formattedText: String?
    let timestamp: Date
    let appName: String
    let appBundleId: String
    let duration: Double
    let numWords: Int
    let language: String
}

struct TranscriptResult {
    let id: String
    let asrText: String?
    let formattedText: String?
    let duration: Double
    let numWords: Int
}
