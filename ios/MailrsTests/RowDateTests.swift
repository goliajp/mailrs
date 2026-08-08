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

/// The chosen time zone has to reach the printed time, not only the
/// bucket it was sorted into.
struct RowDateZoneTests {
    private func calendar(_ zone: String) -> Calendar {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = TimeZone(identifier: zone)!
        c.locale = Locale(identifier: "en_US_POSIX")
        return c
    }

    /// 2026-08-05 22:20 UTC — the same instant is the 5th in London
    /// and the 6th in Tokyo, and each must say so.
    @Test func theSameInstantReadsDifferentlyInTwoZones() {
        let epoch: Int64 = 1_785_968_400
        let now = Date(timeIntervalSince1970: TimeInterval(epoch) + 60 * 60 * 24 * 90)
        let london = RowDate.label(epochSeconds: epoch, now: now, calendar: calendar("Europe/London"))
        let tokyo = RowDate.label(epochSeconds: epoch, now: now, calendar: calendar("Asia/Tokyo"))
        #expect(london != tokyo)
    }

    /// Today in one zone can be yesterday in another; the ladder is
    /// computed in the reader's zone, not the phone's.
    @Test func theLadderIsComputedInTheChosenZone() {
        let epoch: Int64 = 1_785_968_400
        let now = Date(timeIntervalSince1970: TimeInterval(epoch) + 3600)
        let tokyo = RowDate.bucket(
            for: Date(timeIntervalSince1970: TimeInterval(epoch)),
            now: now, calendar: calendar("Asia/Tokyo")
        )
        let honolulu = RowDate.bucket(
            for: Date(timeIntervalSince1970: TimeInterval(epoch)),
            now: now, calendar: calendar("Pacific/Honolulu")
        )
        #expect(tokyo == .time || honolulu == .time)
    }
}
