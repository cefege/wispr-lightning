import Foundation

enum Constants {
    static let supabaseURL = "https://dodjkfqhwrzqjwkfnthl.supabase.co"
    static let supabaseAnonKey = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImRvZGprZnFod3J6cWp3a2ZudGhsIiwicm9sZSI6ImFub24iLCJpYXQiOjE3MTk4ODQzMDcsImV4cCI6MjAzNTQ2MDMwN30.h6EeQ_6kqFeznH25icVUX0Szn9__kc8HoSXAsxxBWG8"
    static let wsURL = "wss://api.wisprflow.ai/llm/ws"
    static let apiURL = "https://api.wisprflow.ai"
    static let sampleRate = 16000
    static let channels = 1
    static let chunkDurationMs = 40
    static let chunkSamples = sampleRate * chunkDurationMs / 1000  // 640
    static let clientVersion = "1.4.549"
    static let maxRecordingSeconds = 300
    static let warningSeconds = 240
    static let finalWarningSeconds = 270

    // Creator mode durations
    static let creatorMaxRecordingSeconds = 600
    static let creatorWarningSeconds = 540
    static let creatorFinalWarningSeconds = 570
}
