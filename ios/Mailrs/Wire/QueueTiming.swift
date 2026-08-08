import Foundation

/// What a queued message's timestamps mean.
///
/// The wire has carried `scheduled_at`, `next_retry` and `created_at`
/// since the queue screen was written, and the row printed none of
/// them: it said "Waiting", which is what the word *queue* had already
/// said. A queue screen exists to answer two questions — how long has
/// this been sitting here, and when will it move — and neither was on
/// it.
///
/// Worse, a **deliberately scheduled** send and a **stuck** one read
/// identically. One is working as intended and the other needs
/// attention, and the operator could not tell them apart.
enum QueueTiming: Equatable {
    /// Sending right now; no clock is useful.
    case inflight
    /// Not stuck — asked for later, and later has not arrived.
    case scheduled(at: Int64)
    /// Failed at least once, and this is when it tries again.
    case retrying(at: Int64)
    /// Waiting for the sender to reach it.
    case queued(since: Int64)
    /// The server sent no timestamps at all.
    case unknown

    /// `now` is injected: "scheduled" and "overdue" differ only by the
    /// clock, and a rule that reads the clock itself cannot be tested.
    static func of(
        status: String, scheduledAt: Int64?, nextRetry: Int64?, createdAt: Int64?, now: Int64
    ) -> QueueTiming {
        if status == "inflight" { return .inflight }
        // A future scheduled time outranks everything: it explains the
        // wait, so nothing else needs to.
        if let scheduledAt, scheduledAt > now { return .scheduled(at: scheduledAt) }
        // A retry time in the past is not a promise — the sender has not
        // got to it yet, and printing "next attempt 20:17" at 21:00 says
        // the queue is broken when it is merely busy.
        if let nextRetry, nextRetry > now { return .retrying(at: nextRetry) }
        if let createdAt { return .queued(since: createdAt) }
        return .unknown
    }

    /// The moment on the row, if there is one.
    var epochSeconds: Int64? {
        switch self {
        case .scheduled(let at), .retrying(let at): return at
        case .queued(let since): return since
        case .inflight, .unknown: return nil
        }
    }

    /// A scheduled send is not a problem, so it does not wear the
    /// colour of one.
    var isScheduled: Bool {
        if case .scheduled = self { return true }
        return false
    }
}
