import Foundation
import Testing

@testable import Mailrs

/// The POP3 conversation, without a server.
@Suite struct POP3SessionTests {
    private func session(_ lines: [String]) -> (POP3Session, ScriptedTransport) {
        let script = ScriptedTransport(lines)
        return (POP3Session(transport: script), script)
    }

    /// **Both** answers matter. A server that accepts the name and
    /// refuses the password says so only on the second, and a client
    /// that checks one of them signs in to nothing.
    @Test func aRefusedPasswordIsARefusalEvenAfterAnAcceptedUser() async throws {
        let (s, _) = session(["+OK user accepted", "-ERR [AUTH] Invalid password"])
        await #expect(throws: POP3Session.Failure.refused("[AUTH] Invalid password")) {
            try await s.login(user: "me", password: "wrong")
        }
    }

    /// POP3 has no code for "wrong credential", so the words are all
    /// there is — and a refusal that is not about the credential must
    /// not be reported as one, or somebody retypes a password that was
    /// always right.
    @Test func aRefusalThatIsNotAboutTheCredentialIsNotOne() async throws {
        let (s, _) = session(["+OK", "-ERR mailbox locked by another session"])
        await #expect(
            throws: POP3Session.Failure.server("mailbox locked by another session")
        ) {
            try await s.login(user: "me", password: "right")
        }
    }

    /// `UIDL` is the only durable identity POP3 offers: message numbers
    /// are renumbered every session, so a client that remembers numbers
    /// re-downloads the mailbox after any delete made elsewhere.
    @Test func theListingIsReadToItsTerminator() async throws {
        let (s, _) = session([
            "+OK", "1 QhdPYR:00WBw1Ph7x7", "2 QhdPYR:00WBw1Ph7x8", ".",
        ])
        let all = try await s.uidls()
        #expect(all.map(\.number) == [1, 2])
        #expect(all.first?.id == "QhdPYR:00WBw1Ph7x7")
    }

    /// `TOP n 0` — headers and none of the body. Fetching whole
    /// messages to show a list downloads the mailbox to display it, and
    /// on a phone that is somebody's data allowance.
    @Test func headersAreAskedForWithoutTheBody() async throws {
        let (s, script) = session(["+OK", "Subject: hi", "From: a@b", "", "."])
        let head = try await s.headers(number: 3)
        #expect(await script.written.first?.hasPrefix("TOP 3 0") == true)
        #expect(head.contains("Subject: hi"))
    }

    /// A body line that began with `.` arrives doubled. A client that
    /// does not undo it corrupts every message containing such a line —
    /// and `.` alone ends the response and is not part of the message.
    @Test func dotStuffingIsUndone() async throws {
        let (s, _) = session([
            "+OK", "Subject: x", "", "..hidden dot", "ordinary", ".",
        ])
        let message = SocketText.latin1(try await s.retrieve(number: 1))
        #expect(message.contains("\r\n.hidden dot\r\n"))
        #expect(message.hasSuffix("ordinary"))
    }

    /// A POP3 server holds an exclusive lock on the mailbox for the
    /// length of a session. One dropped without QUIT keeps that lock
    /// until it times out, during which nothing else — including the
    /// person's other device — can read their mail.
    @Test func theSessionIsEndedProperly() async throws {
        let (s, script) = session(["+OK bye"])
        await s.quit()
        #expect(await script.written.contains { $0.hasPrefix("QUIT") })
    }

    /// A greeting that is not a greeting is not a connection.
    @Test func aRefusedConnectionIsNotAConnection() async throws {
        let (s, _) = session(["-ERR server busy, try later"])
        await #expect(throws: POP3Session.Failure.server("server busy, try later")) {
            try await s.connect()
        }
    }
}
