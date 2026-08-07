import Foundation
import Testing

@testable import Mailrs

struct RowDateTests {
    // A fixed "now": 2026-08-07 12:00 UTC, in a fixed calendar so the
    // ladder is asserted, not the machine's timezone.
    let calendar: Calendar = {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = TimeZone(identifier: "UTC")!
        return c
    }()
    let now = Date(timeIntervalSince1970: 1786190400)

    private func bucket(hoursAgo: Double) -> RowDate.Bucket {
        RowDate.bucket(for: now.addingTimeInterval(-hoursAgo * 3600), now: now, calendar: calendar)
    }

    @Test func todayShowsTheTime() {
        #expect(bucket(hoursAgo: 1) == .time)
        #expect(bucket(hoursAgo: 11) == .time)
    }

    @Test func yesterdayIsNamed() {
        #expect(bucket(hoursAgo: 24) == .yesterday)
    }

    @Test func thisWeekIsAWeekday() {
        #expect(bucket(hoursAgo: 3 * 24) == .weekday)
        #expect(bucket(hoursAgo: 6 * 24) == .weekday)
    }

    @Test func thisYearIsMonthAndDay() {
        #expect(bucket(hoursAgo: 30 * 24) == .sameYearDate)
    }

    @Test func olderYearsCarryTheYear() {
        #expect(bucket(hoursAgo: 400 * 24) == .datedWithYear)
    }

    /// The formatter half, smoke-level: today's label is a clock, not a
    /// date — the exact information "Aug 5" was hiding.
    @Test func todayLabelIsAClock() {
        let label = RowDate.label(
            epochSeconds: Int64(now.timeIntervalSince1970) - 3600, now: now, calendar: calendar
        )
        #expect(label.contains(":"), "expected a time, got \(label)")
    }
}
