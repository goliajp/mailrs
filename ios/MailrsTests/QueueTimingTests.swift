import Foundation
import Testing

@testable import Mailrs

/// The distinction the queue screen could not draw: a message asked for
/// later, and a message that should already have gone.
struct QueueTimingTests {
    private let now: Int64 = 1_786_190_400

    private func timing(
        status: String = "pending", scheduled: Int64? = nil,
        retry: Int64? = nil, created: Int64? = nil
    ) -> QueueTiming {
        QueueTiming.of(status: status, scheduledAt: scheduled, nextRetry: retry,
                       createdAt: created, now: now)
    }

    @Test func inflightNeedsNoClock() {
        #expect(timing(status: "inflight", created: now - 60) == .inflight)
    }

    /// The whole point: both of these were "Waiting" before.
    @Test func aScheduledSendIsNotAStuckOne() {
        let future = timing(scheduled: now + 3600, created: now - 60)
        let stuck = timing(retry: now + 300, created: now - 7200)
        #expect(future == .scheduled(at: now + 3600))
        #expect(stuck == .retrying(at: now + 300))
        #expect(future.isScheduled)
        #expect(!stuck.isScheduled)
    }

    /// A scheduled time that has passed explains nothing — the message
    /// is late, and the row should fall back to why it is waiting.
    @Test func aPastScheduleStopsExplainingTheWait() {
        #expect(timing(scheduled: now - 60, created: now - 3600) == .queued(since: now - 3600))
    }

    /// Printing "next attempt 20:17" at 21:00 accuses the queue of being
    /// broken when it is only busy.
    @Test func aRetryTimeInThePastIsNotShownAsAPromise() {
        #expect(timing(retry: now - 60, created: now - 3600) == .queued(since: now - 3600))
    }

    /// Scheduling outranks a retry: it explains the wait on its own.
    @Test func aFutureScheduleOutranksARetry() {
        #expect(timing(scheduled: now + 7200, retry: now + 300) == .scheduled(at: now + 7200))
    }

    @Test func withoutTimestampsThereIsNothingToShow() {
        #expect(timing() == .unknown)
        #expect(timing().epochSeconds == nil)
    }

    /// Every case that claims a moment can produce one, or the row
    /// renders an icon and a phrase with nothing in it.
    @Test func everyTimedCaseCarriesItsMoment() {
        #expect(timing(scheduled: now + 60).epochSeconds == now + 60)
        #expect(timing(retry: now + 60).epochSeconds == now + 60)
        #expect(timing(created: now - 60).epochSeconds == now - 60)
        #expect(timing(status: "inflight").epochSeconds == nil)
    }
}
