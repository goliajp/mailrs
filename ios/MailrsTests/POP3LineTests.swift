import Testing

@testable import Mailrs

/// Reading what a POP3 server says.
@Suite struct POP3LineTests {
    @Test func okAndErrAreToldApart() {
        #expect(POP3.reply("+OK 2 messages") == POP3.Reply(ok: true, text: "2 messages"))
        #expect(POP3.reply("-ERR bad password") == POP3.Reply(ok: false, text: "bad password"))
        #expect(POP3.reply("+OK") == POP3.Reply(ok: true, text: ""))
        #expect(POP3.reply("nonsense") == nil)
    }

    /// Message numbers are renumbered every session; the uidl is the
    /// only thing that survives. A client that remembers numbers
    /// re-downloads the mailbox after any delete made elsewhere.
    @Test func aUidlLineIsANumberAndAnIdentity() {
        #expect(POP3.uidl("3 QhdPYR:00WBw1Ph7x7") == POP3.Uidl(number: 3, id: "QhdPYR:00WBw1Ph7x7"))
    }

    /// A uidl may hold anything printable, including spaces on some
    /// servers — so the split is on the **first** space only.
    @Test func aUidlWithASpaceSurvives() {
        #expect(POP3.uidl("7 abc def")?.id == "abc def")
    }

    @Test func aLineThatIsNotAUidlIsNotGuessedAt() {
        #expect(POP3.uidl("3") == nil)
        #expect(POP3.uidl("x abc") == nil)
        #expect(POP3.uidl("3 ") == nil)
    }

    /// The mirror of SMTP's dot-stuffing. A client that does not undo
    /// it corrupts every message with a line starting `.`.
    @Test func dotStuffingIsUndone() {
        #expect(POP3.unstuffed(["first", "..hidden", "last", "."])
            == "first\r\n.hidden\r\nlast")
    }

    /// `.` alone ends the response and is not part of the message.
    @Test func theTerminatorIsNotPartOfTheMessage() {
        #expect(POP3.unstuffed(["body", ".", "after"]) == "body")
    }

    /// A dot inside a line is not stuffing and must not be eaten.
    @Test func aDotInsideALineIsLeftAlone() {
        #expect(POP3.unstuffed(["see fig. 1", "."]) == "see fig. 1")
    }

    @Test func aRefusedCredentialIsRecognisedFromTheWordsAlone() {
        #expect(POP3.isAuthenticationFailure("authentication failed"))
        #expect(POP3.isAuthenticationFailure("invalid username or password"))
        #expect(!POP3.isAuthenticationFailure("server busy, try again later"))
    }
}
