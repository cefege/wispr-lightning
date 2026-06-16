import Foundation

/// "Fire exactly once" wrapper used to safely race multiple completion paths
/// for a single async operation — provider success, watchdog timeout, user
/// cancel, error fallback, etc. Three previous ad-hoc implementations of this
/// pattern (NSLock+Bool+closure variants in WisprFlowProvider, DeepgramProvider,
/// AppDelegate) were prone to drift; consolidate here so any future fix lands
/// in one place.
///
/// Usage:
///     let gate = SafeCompletion<Result<X, Y>> { result in
///         // runs at most once, on whatever thread fired first
///     }
///     gate.fire(.success(value))   // body runs
///     gate.fire(.failure(error))   // no-op (first call already won)
///
/// Thread-safe. Drops its body reference after firing so captured state is
/// released even if callers keep the gate alive.
final class SafeCompletion<Value> {
    private let lock = NSLock()
    private var hasFired = false
    private var body: ((Value) -> Void)?

    init(_ body: @escaping (Value) -> Void) {
        self.body = body
    }

    /// Deliver `value` to the body if this is the first call. Subsequent
    /// calls are silently dropped. Body runs OUTSIDE the lock so it can do
    /// arbitrary work (call into UI, schedule timers, etc.) without
    /// risking re-entrant deadlock.
    func fire(_ value: Value) {
        lock.lock()
        let runBody: ((Value) -> Void)?
        if hasFired {
            runBody = nil
        } else {
            hasFired = true
            runBody = body
            body = nil
        }
        lock.unlock()
        runBody?(value)
    }

    var hasCompleted: Bool {
        lock.lock()
        defer { lock.unlock() }
        return hasFired
    }
}
