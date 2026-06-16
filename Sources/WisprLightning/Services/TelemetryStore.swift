import Foundation

/// Per-dictation telemetry. Shown in the status-bar submenu so the user can
/// see at a glance whether the fallback chain / watchdog / retry machinery
/// is doing anything in practice — without the safety nets being visible,
/// "the system works" and "the system silently lost a fallback hop" look
/// identical until the next incident.
struct AttemptRecord: Identifiable {
    let id: UUID
    let timestamp: Date
    /// Display name of the vendor that produced the final text, or nil if
    /// no provider succeeded.
    let finalVendor: String?
    /// 0 = primary vendor returned text. 1 = first fallback hop. etc.
    let fallbackHops: Int
    /// Whether the per-provider watchdog timer fired during this attempt
    /// (means at least one provider hung past its budget).
    let watchdogFired: Bool
    let elapsedSeconds: Double
    let outcome: Outcome
    /// First ~60 chars of the transcript on success; the error message on
    /// failure; nil on cancel.
    let preview: String?

    enum Outcome {
        case success
        case failure
        case cancelled
    }

    var symbol: String {
        switch outcome {
        case .success:   return "✓"
        case .failure:   return "✗"
        case .cancelled: return "⊘"
        }
    }
}

/// Bounded ring buffer of recent attempts. Thread-safe — readers (status bar
/// menu build) and writers (AppDelegate completion paths) can hit it from
/// different queues.
final class TelemetryStore {
    private let lock = NSLock()
    private var records: [AttemptRecord] = []
    private let maxRecords: Int

    init(maxRecords: Int = 10) {
        self.maxRecords = maxRecords
    }

    func record(_ record: AttemptRecord) {
        lock.lock()
        records.insert(record, at: 0)
        if records.count > maxRecords {
            records.removeLast(records.count - maxRecords)
        }
        lock.unlock()
        DispatchQueue.main.async {
            NotificationCenter.default.post(name: .telemetryUpdated, object: nil)
        }
    }

    func recent() -> [AttemptRecord] {
        lock.lock()
        defer { lock.unlock() }
        return records
    }
}

extension Notification.Name {
    static let telemetryUpdated = Notification.Name("WisprLightningTelemetryUpdated")
}
