import Foundation

/// A continuation that may be resumed from several callbacks, of which
/// only the first counts.
///
/// `NWConnection.stateUpdateHandler` fires repeatedly and off-queue,
/// and resuming a continuation twice is a crash rather than a warning.
final class ResumeOnce: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false

    func resume(_ k: CheckedContinuation<Void, Error>, with result: Result<Void, Error>) {
        lock.lock()
        let first = !done
        done = true
        lock.unlock()
        guard first else { return }
        k.resume(with: result)
    }
}
