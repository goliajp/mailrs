import Foundation
import Testing

@testable import Mailrs

@Suite("Snooze")
struct SnoozeChoiceTests {
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

    private func parts(_ date: Date) -> (weekday: Int, hour: Int) {
        let c = calendar.dateComponents([.weekday, .hour], from: date)
        return (c.weekday ?? 0, c.hour ?? -1)
    }

    /// A thread that comes back in the middle of the night is worse
    /// than one that never left.
    @Test("every morning choice lands at 8am where the phone is")
    func mornings() {
        let now = at("2026-08-10T23:40:00+09:00")
        for choice in [SnoozeChoice.tomorrowMorning, .thisWeekend, .nextWeek] {
            let fire = choice.date(after: now, calendar: calendar)
            #expect(parts(fire ?? .distantPast).hour == 8, "\(choice.label) is not a morning")
            #expect(fire ?? .distantPast > now, "\(choice.label) is in the past")
        }
    }

    @Test("the weekend is a Saturday and next week is a Monday")
    func weekdays() {
        let now = at("2026-08-12T10:00:00+09:00")
        #expect(parts(SnoozeChoice.thisWeekend.date(after: now, calendar: calendar)!).weekday == 7)
        #expect(parts(SnoozeChoice.nextWeek.date(after: now, calendar: calendar)!).weekday == 2)
    }

    /// Said on a Monday, "next week" means the Monday coming — not one
    /// that began nine hours ago.
    @Test("a weekday choice is never today")
    func neverToday() {
        let monday = at("2026-08-10T09:00:00+09:00")
        let fire = SnoozeChoice.nextWeek.date(after: monday, calendar: calendar)!
        #expect(fire > monday)
        #expect(fire.timeIntervalSince(monday) > 6 * 24 * 60 * 60)
    }

    @Test("later today is three hours on, even at night")
    func laterToday() {
        let now = at("2026-08-10T23:40:00+09:00")
        #expect(SnoozeChoice.laterToday.date(after: now, calendar: calendar)
            == now.addingTimeInterval(3 * 60 * 60))
    }

    /// A server older than v2.55 does not send the field, and absent
    /// means awake — not asleep since 1970.
    @Test("a row without the field is awake")
    func absentIsAwake() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        #expect(!SnoozeState.isAsleep(row(snoozedUntil: nil), now: now))
        #expect(!SnoozeState.isAsleep(row(snoozedUntil: 0), now: now))
        #expect(!SnoozeState.isAsleep(row(snoozedUntil: 1_700_000_000), now: now))
        #expect(SnoozeState.isAsleep(row(snoozedUntil: 1_900_000_000), now: now))
    }

    private func row(snoozedUntil: Int64?) -> Wire.Conversation {
        Wire.Conversation(
            threadId: "t", subject: "s", participants: [], messageCount: 1,
            unreadCount: 0, lastDate: 0, category: "inbox", flagged: false,
            snippet: "", pinned: false, archived: false, snoozedUntil: snoozedUntil,
            importanceLevel: "low", importanceScore: 0, requiresAction: false,
            receivedCount: 1, sentCount: 0)
    }
}
