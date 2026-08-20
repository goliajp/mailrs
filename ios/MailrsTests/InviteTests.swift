import Foundation
import Testing

@testable import Mailrs

struct InviteTests {
    /// The case that matters, because it is the common one: Exchange
    /// does not send `METHOD:UPDATE`. It re-sends the whole invitation
    /// as a `REQUEST` with a higher `SEQUENCE`, so a meeting moved nine
    /// times arrives as SEQUENCE 9 — and calling that a new invitation
    /// tells the reader the opposite of what happened.
    @Test func aResentRequestIsAnUpdate() {
        #expect(InviteMethod.badge("REQUEST", sequence: 9) == "Updated invite")
        #expect(InviteMethod.badge("REQUEST", sequence: 0) == "New invite")
        #expect(InviteMethod.badge("CANCEL", sequence: 3) == "Cancelled")
    }

    /// Offering Yes/No against a `PUBLISH` or somebody else's `REPLY`
    /// sends an iTIP message to a party that never asked for one.
    @Test func onlyARequestAsksAnything() {
        #expect(InviteMethod.wantsAnswer("REQUEST"))
        #expect(!InviteMethod.wantsAnswer("PUBLISH"))
        #expect(!InviteMethod.wantsAnswer("REPLY"))
        #expect(!InviteMethod.wantsAnswer("CANCEL"))
    }

    /// The instant is the server's, resolved against the invitation's
    /// own VTIMEZONE. This side must read it and not the wall-clock:
    /// `Pacific Standard Time` says "Standard" while the event is in
    /// daylight time, and reading the name gives an hour that is wrong
    /// for half the year — reading the wall-clock as UTC gives one that
    /// is seven hours wrong all year.
    @Test func theInstantComesFromTheServer() throws {
        let json = """
        {
          "uid": "u1", "sequence": 9, "summary": "Product sync",
          "location": "H-120", "status": "CONFIRMED",
          "organizer": {"cn": "Chair", "email": "chair@example.com"},
          "attendees": [{"email": "me@example.com", "partstat": "NEEDS-ACTION"}],
          "dtstart": {"Zoned": {"local": "2026-08-20T16:00:00", "tz_name": "Pacific Standard Time"}},
          "dtstart_utc": "2026-08-20T23:00:00+00:00",
          "dtend_utc": "2026-08-20T23:50:00+00:00"
        }
        """
        let invite = try JSONDecoder().decode(Wire.Invite.self, from: Data(json.utf8))
        #expect(invite.startsAt?.timeIntervalSince1970 == 1_787_266_800)
        #expect(invite.organiserZone == "Pacific Standard Time")
        #expect(invite.organiserWallClock == "2026-08-20T16:00:00")
        #expect(invite.sequence == 9)
        #expect(invite.attendees.count == 1)
    }

    /// An all-day event has no instant, and giving it one moves it a
    /// day for readers west of the organiser.
    @Test func anAllDayEventHasNoInstant() throws {
        let json = """
        {"uid": "u2", "sequence": 0, "summary": "Holiday",
         "dtstart": {"Date": "2026-08-20"}}
        """
        let invite = try JSONDecoder().decode(Wire.Invite.self, from: Data(json.utf8))
        #expect(invite.startsAt == nil)
        #expect(invite.organiserZone == nil)
    }

    /// A count answers "how many"; the states answer "is this
    /// happening", which is the question somebody deciding whether to
    /// go actually has.
    @Test func theGuestLineSaysWhoIsComing() throws {
        let json = """
        {"uid": "u3", "sequence": 0, "summary": "s", "attendees": [
          {"email": "a@x.com", "partstat": "ACCEPTED"},
          {"email": "b@x.com", "partstat": "NEEDS-ACTION"},
          {"email": "c@x.com", "partstat": "NEEDS-ACTION"}
        ]}
        """
        let invite = try JSONDecoder().decode(Wire.Invite.self, from: Data(json.utf8))
        #expect(InviteGuests.summary(invite.attendees) == "3 guests · 1 yes, 2 awaiting")
        #expect(InviteGuests.answered("ACCEPTED") == "You accepted")
    }

    /// Mail with no calendar part must not claim to carry one — the
    /// assertion that stops the badge appearing on everything.
    @Test func ordinaryMailCarriesNoInvitation() throws {
        let json = """
        {"uid": 7, "sender": "a@x.com", "recipients": "me@x.com", "subject": "hi",
         "internal_date": 1786000000, "message_id": "m1@x.com"}
        """
        let message = try JSONDecoder().decode(Wire.Message.self, from: Data(json.utf8))
        #expect(message.inviteMethod.isEmpty)
    }
}
