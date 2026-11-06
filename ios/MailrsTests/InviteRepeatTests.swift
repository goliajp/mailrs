import Foundation
import Testing

@testable import Mailrs

/// The Teams invitation from the bug report — weekly on Thursday at 22:00
/// Pacific, until 2027-04-29 — as the server serves it, with the next
/// occurrence either side of the US clock change on 1 November. The shapes
/// follow `crates/core-sidestate/tests/recurring_rows.rs`.
struct InviteRepeatTests {
    private let english = Locale(identifier: "en_US")
    private let chinese = Locale(identifier: "zh_CN")
    private let tokyo = TimeZone(identifier: "Asia/Tokyo")!
    private let berlin = TimeZone(identifier: "Europe/Berlin")!

    private static let beforeChange = """
        {"start":"2026-10-30T05:00:00+00:00","end":"2026-10-30T06:00:00+00:00",
         "organiser_local":"2026-10-29T22:00:00","organiser_offset_secs":-25200}
        """
    private static let afterChange = """
        {"start":"2026-11-06T06:00:00+00:00","end":"2026-11-06T07:00:00+00:00",
         "organiser_local":"2026-11-05T22:00:00","organiser_offset_secs":-28800}
        """

    private func invite(next: String?) throws -> Wire.Invite {
        let json = """
        {"uid":"golia-sync","sequence":0,"summary":"GOLIA sync","attendees":[],
         "dtstart":{"Zoned":{"tz_name":"Pacific Standard Time","local":"2026-10-29T22:00:00"}},
         "dtstart_utc":"2026-10-30T05:00:00+00:00","dtend_utc":"2026-10-30T06:00:00+00:00",
         "organiser_zone":"America/Los_Angeles",
         "recurrence":{"freq":"WEEKLY","interval":1,"by_day":["TH"],"by_month":[],"by_month_day":[],
                       "count":null,"until":"2027-04-29","plain":true},
         "next_occurrence":\(next ?? "null")}
        """
        return try JSONDecoder().decode(Wire.Invite.self, from: Data(json.utf8))
    }

    private func hour(_ line: String?, _ h24: String, _ h12: String) -> Bool {
        guard let line else { return false }
        return line.contains(h24) || line.contains(h12)
    }

    /// Tokyo has no daylight time: 14:00 before the US change, 15:00 after.
    @Test func theNextMeetingInTokyoMovesAnHourWhenTheUSClocksGoBack() throws {
        let before = InviteRepeat.whenLine(try invite(next: Self.beforeChange), reader: tokyo, locale: english)
        #expect(hour(before, "14:00", "2:00"))
        #expect(before?.hasPrefix("Next:") == true)
        #expect(before?.hasSuffix("PDT") == true)
        let after = InviteRepeat.whenLine(try invite(next: Self.afterChange), reader: tokyo, locale: english)
        #expect(hour(after, "15:00", "3:00"))
        #expect(after?.hasSuffix("PST") == true)
        #expect(after?.contains("Pacific Standard Time") == false)
    }

    /// Berlin goes back a week before the US: 06:00 in the week between,
    /// 07:00 once both have changed.
    @Test func theNextMeetingInBerlinFollowsBothChanges() throws {
        #expect(hour(InviteRepeat.whenLine(try invite(next: Self.beforeChange), reader: berlin, locale: english),
                     "06:00", "6:00"))
        #expect(hour(InviteRepeat.whenLine(try invite(next: Self.afterChange), reader: berlin, locale: english),
                     "07:00", "7:00"))
    }

    @Test func theRuleReadsInTheReadersLanguage() throws {
        let series = try invite(next: Self.afterChange)
        #expect(InviteRepeat.ruleLine(series, reader: tokyo, locale: english)
            == "Weekly on Thursday · until Apr 29, 2027 (organizer’s time)")
        #expect(InviteRepeat.ruleLine(series, reader: tokyo, locale: chinese)
            == "每周四 · 到 2027年4月29日 为止（按组织者的时间）")
    }

    @Test func aReaderInTheOrganisersZoneIsNotToldItTwice() throws {
        let la = TimeZone(identifier: "America/Los_Angeles")!
        let series = try invite(next: Self.afterChange)
        #expect(InviteRepeat.whenLine(series, reader: la, locale: english)?.contains("PST") == false)
        #expect(InviteRepeat.ruleLine(series, reader: la, locale: english)
            == "Weekly on Thursday · until Apr 29, 2027")
    }

    @Test func aSeriesThatIsOverSaysSo() throws {
        let ended = try invite(next: nil)
        #expect(InviteRepeat.ruleLine(ended, reader: tokyo, locale: english)?.hasSuffix("the series has ended") == true)
        #expect(InviteRepeat.whenLine(ended, reader: tokyo, locale: english)?.hasPrefix("Next:") == false)
    }

    @Test func aSingleEventHasNoRuleLineAndTheAbbreviationOfItsDate() throws {
        let json = """
        {"uid":"x","summary":"x","attendees":[],"dtstart_utc":"2026-10-30T05:00:00+00:00",
         "organiser_zone":"America/Los_Angeles"}
        """
        let single = try JSONDecoder().decode(Wire.Invite.self, from: Data(json.utf8))
        #expect(InviteRepeat.ruleLine(single, reader: tokyo, locale: english) == nil)
        // 30 October 05:00 UTC is daylight time in Santa Clara
        #expect(InviteRepeat.whenLine(single, reader: tokyo, locale: english)?.hasSuffix("PDT") == true)
    }
}
