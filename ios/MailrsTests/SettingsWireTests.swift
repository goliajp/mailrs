import Foundation
import Testing

@testable import Mailrs

/// The same cases as Android's `SettingsWireTest`.
@Suite("Settings wire")
struct SettingsWireTests {
    @Test func imagesFollowTheSetting() {
        #expect(!RemoteImages.blocked(.auto, loaded: false))
        #expect(RemoteImages.blocked(.click, loaded: false))
        #expect(!RemoteImages.blocked(.click, loaded: true))
        #expect(RemoteImages.blocked(.never, loaded: true))
        #expect(RemoteImages.offersButton(.click, loaded: false))
        #expect(!RemoteImages.offersButton(.click, loaded: true))
        #expect(!RemoteImages.offersButton(.never, loaded: false))
        #expect(!RemoteImages.offersButton(.auto, loaded: false))
    }

    /// Absent fields are the server's defaults, as `Prefs` reads them.
    @Test func notifyPrefsReadAbsentFieldsAsTheServerDoes() throws {
        let p = try JSONDecoder().decode(SettingsWire.NotifyPrefs.self, from: Data("{}".utf8))
        #expect(p.mode == "default")
        #expect(p.quietStart == nil && p.quietEnd == nil)
        #expect(p.utcOffsetMinutes == 0 && !p.hideContent)
    }

    /// Quiet hours off sends neither end, which is the only other window the server takes.
    @Test func quietHoursOffSendsNeitherEnd() throws {
        var p = try JSONDecoder().decode(SettingsWire.NotifyPrefs.self, from: Data(#"{"quiet_start":1320,"quiet_end":420}"#.utf8))
        p.quietStart = QuietHours.start(on: false)
        p.quietEnd = QuietHours.end(on: false)
        let sent = String(decoding: try JSONEncoder().encode(p), as: UTF8.self)
        #expect(!sent.contains("quiet_start") && !sent.contains("quiet_end"))
        #expect(QuietHours.start(on: true) == 1320 && QuietHours.end(on: true) == 420)
    }

    @Test func quietHoursAreMinutesAfterMidnight() {
        #expect(QuietHours.minutesAfterMidnight(of: QuietHours.date(minutes: 22 * 60 + 15)) == 1335)
    }

    /// Going back to the rule is `null`, written out as the web writes it.
    @Test func clearingAnOverrideSendsNull() throws {
        let sent = String(decoding: try JSONEncoder().encode(OverrideBody(value: nil)), as: UTF8.self)
        #expect(sent == #"{"value":null}"#)
    }

    @Test func aVacationWithNoTextCannotBeTurnedOn() throws {
        let blank = #"{"enabled":false,"subject":"","body":"  ","start":null,"end":null,"days":7,"version":0}"#
        let v = try JSONDecoder().decode(SettingsWire.Vacation.self, from: Data(blank.utf8))
        #expect(!v.canTurnOn)
    }

    @Test func appPasswordsAreMadeOnTheWebSecurityPage() {
        let url = SettingsWire.appPasswordsPage(server: URL(string: "https://mail.golia.jp")!)
        #expect(url.absoluteString == "https://mail.golia.jp/settings?tab=security")
    }
}
