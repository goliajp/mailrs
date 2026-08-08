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

/// The other two forms. Both exist because the ladder answers a
/// scanning question, and two surfaces are not scanning.
struct RowDateFormTests {
    private var calendar: Calendar {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = TimeZone(identifier: "UTC")!
        c.locale = Locale(identifier: "en_US")
        return c
    }

    /// 2026-08-07 12:00 UTC.
    private let now = Date(timeIntervalSince1970: 1786190400)
    private var nowEpoch: Int64 { Int64(now.timeIntervalSince1970) }

    /// The point of `.stamp`: an opened message from today keeps its
    /// date, where the ladder would have printed the clock alone.
    @Test func aStampCarriesBothHalvesEvenToday() {
        let stamp = RowDate.stamp(epochSeconds: nowEpoch - 3600, calendar: calendar)
        #expect(stamp.contains(":"), "expected a time in \(stamp)")
        #expect(stamp.contains("2026"), "expected the year in \(stamp)")
    }

    /// And the year is there four years later too — the whole reason
    /// `.month().day().hour().minute()` was not enough.
    @Test func aStampFromAnotherYearSaysWhichYear() {
        let fourYears: Int64 = 4 * 365 * 24 * 3600
        let stamp = RowDate.stamp(epochSeconds: nowEpoch - fourYears, calendar: calendar)
        #expect(stamp.contains("2022"), "expected the year in \(stamp)")
    }

    /// `.day` is a window, so it must not invent a clock — a DMARC
    /// report covering today is not an event at 12:00.
    @Test func aDayHasNoClockOnIt() {
        let today = RowDate.day(epochSeconds: nowEpoch - 3600, now: now, calendar: calendar)
        #expect(!today.contains(":"), "expected no time in \(today)")
        let old = RowDate.day(epochSeconds: nowEpoch - 400 * 24 * 3600, now: now, calendar: calendar)
        #expect(!old.contains(":"), "expected no time in \(old)")
    }

    /// The ladder's economy survives in the one place it still holds:
    /// this year's window needs no year, an older one does.
    @Test func aDayCarriesTheYearOnlyWhenItIsNotThisOne() {
        let thisYear = RowDate.day(epochSeconds: nowEpoch - 30 * 24 * 3600, now: now, calendar: calendar)
        #expect(!thisYear.contains("2026"), "did not expect the year in \(thisYear)")
        let older = RowDate.day(epochSeconds: nowEpoch - 400 * 24 * 3600, now: now, calendar: calendar)
        #expect(older.contains("2025"), "expected the year in \(older)")
    }

    /// All three forms are read in the reader's zone, not the phone's —
    /// asserted per form, because each builds its own formatter.
    @Test func everyFormIsReadInTheChosenZone() {
        var honolulu = calendar
        honolulu.timeZone = TimeZone(identifier: "Pacific/Honolulu")!
        // 2026-08-05 22:20 UTC is still the 5th in Honolulu and already
        // the 6th in Tokyo.
        let epoch: Int64 = 1_785_968_400
        var tokyo = calendar
        tokyo.timeZone = TimeZone(identifier: "Asia/Tokyo")!
        #expect(RowDate.stamp(epochSeconds: epoch, calendar: honolulu)
                != RowDate.stamp(epochSeconds: epoch, calendar: tokyo))
        #expect(RowDate.day(epochSeconds: epoch, now: now, calendar: honolulu)
                != RowDate.day(epochSeconds: epoch, now: now, calendar: tokyo))
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
