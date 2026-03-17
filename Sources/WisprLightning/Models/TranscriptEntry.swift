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

enum TranscriptionError: Error {
    case authFailed
    case connectionFailed
    case serverError(String)
    case timeout
    case emptyResult

    var isRetryable: Bool {
        switch self {
        case .connectionFailed, .timeout, .serverError:
            return true
        case .authFailed, .emptyResult:
            return false
        }
    }

    var userMessage: String {
        switch self {
        case .authFailed:
            return "Authentication failed — please sign in again"
        case .connectionFailed:
            return "Connection failed — check your network"
        case .serverError(let detail):
            return "Server error: \(detail)"
        case .timeout:
            return "Request timed out — server did not respond"
        case .emptyResult:
            return "No transcription returned"
        }
    }
}
