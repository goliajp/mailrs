import Foundation
import Testing

@testable import Mailrs

/// An all-day invitation's days: shown as written, never moved by a zone.
struct InviteDaysTests {
    private let english = Locale(identifier: "en_US")

    @Test func oneDayIsOneDate() {
        #expect(InviteDays.line(firstDay: "2026-11-03", dayAfterLast: "2026-11-04", locale: english)
            == "Nov 3, 2026")
    }

    /// DTEND is the day after the last, so the 5th means through the 4th.
    @Test func severalDaysEndOnTheLastDayNotTheOneAfter() {
        #expect(InviteDays.line(firstDay: "2026-11-03", dayAfterLast: "2026-11-05", locale: english)
            == "Nov 3, 2026 – Nov 4, 2026")
    }

    @Test func noEndIsTheFirstDayAlone() {
        #expect(InviteDays.line(firstDay: "2026-11-03", dayAfterLast: nil, locale: english)
            == "Nov 3, 2026")
    }

    @Test func noDayIsNoLine() {
        #expect(InviteDays.line(firstDay: nil, dayAfterLast: nil) == nil)
        #expect(InviteDays.line(firstDay: "not a date", dayAfterLast: nil) == nil)
    }

    /// The payload as `crates/intake/src/invite.rs` files an all-day
    /// invitation: no instants, the dates beside them.
    @Test func anAllDayPayloadDecodesItsDays() throws {
        let json = Data("""
        {"uid":"d1","sequence":0,"summary":"Offsite","attendees":[],
         "dtstart":{"Date":"2026-11-03"},"dtstart_utc":null,"dtend_utc":null,
         "dtstart_date":"2026-11-03","dtend_date":"2026-11-05"}
        """.utf8)
        let invite = try JSONDecoder().decode(Wire.Invite.self, from: json)
        #expect(invite.startsAt == nil)
        #expect(invite.firstDay == "2026-11-03")
        #expect(invite.dayAfterLast == "2026-11-05")
        #expect(invite.organiserZone == nil)
    }
}
