import Foundation
import Testing

@testable import Mailrs

@Suite("Send later")
struct SendScheduleTests {
    /// A fixed calendar in a fixed zone: "morning" that depends on where
    /// the test runs is a test that passes in Tokyo and fails in London.
    private var calendar: Calendar {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = TimeZone(identifier: "Asia/Tokyo") ?? .gmt
        return cal
    }

    private func at(_ iso: String) -> Date {
        let parser = ISO8601DateFormatter()
        parser.timeZone = TimeZone(identifier: "Asia/Tokyo")
        parser.formatOptions = [.withInternetDateTime]
        return parser.date(from: iso) ?? .distantPast
    }

    private func parts(_ date: Date) -> (weekday: Int, hour: Int, day: Int) {
        let c = calendar.dateComponents([.weekday, .hour, .day], from: date)
        return (c.weekday ?? 0, c.hour ?? -1, c.day ?? 0)
    }

    @Test("now is the absence of a schedule, not a time")
    func now() {
        #expect(SendSchedule.now.fireDate(after: at("2026-08-10T23:40:00+09:00"),
                                          calendar: calendar) == nil)
    }

    /// Three hours on, not an evening clock time: chosen at 11pm there
    /// is no evening left, and rolling to tomorrow is a different choice
    /// than the one that was made.
    @Test("later today is three hours on, even at night")
    func laterToday() {
        let now = at("2026-08-10T23:40:00+09:00")
        let fire = SendSchedule.laterToday.date(after: now, calendar: calendar)
        #expect(fire == now.addingTimeInterval(3 * 60 * 60))
    }

    @Test("tomorrow morning is 8am where the phone is")
    func tomorrow() {
        let fire = SendSchedule.tomorrowMorning.date(
            after: at("2026-08-10T23:40:00+09:00"), calendar: calendar)
        let got = parts(fire ?? .distantPast)
        #expect(got.hour == 8)
        #expect(got.day == 11)
    }

    /// Said on a Monday it means the one coming — a schedule that fires
    /// an hour after it was set is not what "Monday morning" promised.
    @Test("monday morning is never today")
    func monday() {
        for iso in ["2026-08-10T00:01:00+09:00", "2026-08-10T13:00:00+09:00",
                    "2026-08-14T09:00:00+09:00"] {
            let fire = SendSchedule.mondayMorning.date(after: at(iso), calendar: calendar)
            let got = parts(fire ?? .distantPast)
            #expect(got.weekday == 2, "not a Monday from \(iso)")
            #expect(got.hour == 8)
            #expect(fire ?? .distantPast > at(iso), "already past, from \(iso)")
        }
    }

    /// Epoch seconds, integral. The web posted ISO 8601 into the same
    /// field and the handler read it as "not scheduling" — every
    /// scheduled send went out at once and nothing said so.
    @Test("the wire value is epoch seconds")
    func epochSeconds() {
        let now = at("2026-08-10T23:40:00+09:00")
        let stamp = SendSchedule.laterToday.fireDate(after: now, calendar: calendar)
        #expect(stamp == Int64(now.timeIntervalSince1970) + 10_800)
    }

    @Test("every option is offered with a name")
    func labelled() {
        #expect(SendSchedule.allCases.count == 4)
        for option in SendSchedule.allCases {
            #expect(!option.label.isEmpty)
        }
    }
}
