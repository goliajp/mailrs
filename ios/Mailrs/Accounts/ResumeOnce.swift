import Foundation

/// A continuation that may be resumed from several callbacks, of which
/// only the first counts.
///
/// `NWConnection.stateUpdateHandler` fires repeatedly and off-queue,
/// and resuming a continuation twice is a crash rather than a warning.
final class ResumeOnce: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false

    /// - Returns: whether **this** call was the one that answered.
    ///   A caller that has cleaning up to do — cancelling the
    ///   connection a timeout gave up on — must do it only when it
    ///   was the one that gave up, or it tears down a connection that
    ///   had already succeeded.
    @discardableResult
    func resume(_ k: CheckedContinuation<Void, Error>, with result: Result<Void, Error>) -> Bool {
        lock.lock()
        let first = !done
        done = true
        lock.unlock()
        guard first else { return false }
        k.resume(with: result)
        return true
    }
}
