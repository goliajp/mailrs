import Foundation
import Testing

@testable import Mailrs

/// The IMAP conversation, without a server.
///
/// Every rule here is a rule about what arrives, and none of them can
/// be checked by connecting to a real server and hoping it sends the
/// awkward case. A scripted transport sends it every time.
@Suite struct IMAPSessionTests {
    private func session(_ lines: [String]) -> (IMAPSession, ScriptedTransport) {
        let script = ScriptedTransport(lines)
        return (IMAPSession(transport: script), script)
    }

    /// `a1` must not be completed by `a10`'s reply. A prefix match here
    /// ends the wrong command, and the rest of that command's output is
    /// read as the next one's.
    @Test func aTagIsMatchedWhole() async throws {
        let (s, script) = session([
            #"* LIST (\HasNoChildren) "." "INBOX""#,
            "a1 OK done",
        ])
        let folders = try await s.list()
        #expect(folders.map(\.name) == ["INBOX"])
        #expect(await script.written.first?.hasPrefix("a1 LIST") == true)
    }

    /// A folder name may contain spaces, and it is at the end of the
    /// line — which is why it is read from the end rather than by
    /// counting fields.
    @Test func aFolderNameWithSpacesSurvives() async throws {
        let (s, _) = session([
            #"* LIST (\HasNoChildren) "/" "[Gmail]/All Mail""#,
            "a1 OK done",
        ])
        #expect(try await s.list().map(\.name) == ["[Gmail]/All Mail"])
    }

    /// SELECT reports where the folder is, and both numbers matter.
    @Test func selectReadsUidValidityAndExists() async throws {
        let (s, _) = session([
            "* 42 EXISTS",
            "* OK [UIDVALIDITY 1234567890] UIDs valid",
            "a1 OK [READ-WRITE] SELECT completed",
        ])
        let (validity, exists) = try await s.select("INBOX")
        #expect(validity == 1_234_567_890)
        #expect(exists == 42)
    }

    /// A literal is read by the byte count the server announced, never
    /// by scanning for a terminator — a message contains every byte
    /// sequence a terminator could be made of, including `)`.
    @Test func aLiteralIsReadByItsAnnouncedLength() async throws {
        let body = "Subject: has a ) in it\r\nFrom: a@b\r\n\r\n"
        let (s, _) = session([
            #"* 1 FETCH (UID 7 FLAGS (\Seen) BODY[HEADER] {\#(body.utf8.count)}"#,
            body + ")",
            "a1 OK done",
        ])
        let fetched = try await s.fetchHeaders(range: "1:*")
        #expect(fetched.count == 1)
        #expect(fetched.first?.uid == 7)
        #expect(fetched.first?.seen == true)
        #expect(fetched.first?.headers.subject == "has a ) in it")
    }

    /// An unread message is unread, and the flag list says so by
    /// absence.
    @Test func absenceOfTheSeenFlagMeansUnread() async throws {
        let body = "Subject: x\r\n\r\n"
        let (s, _) = session([
            #"* 1 FETCH (UID 8 FLAGS () BODY[HEADER] {\#(body.utf8.count)}"#,
            body + ")",
            "a1 OK done",
        ])
        #expect(try await s.fetchHeaders(range: "1:*").first?.seen == false)
    }

    /// `BODY.PEEK[]`, never `BODY[]`: opening a message is the reader's
    /// decision, and a client that marks mail read for having looked at
    /// it takes that decision away.
    @Test func fetchingABodyPeeks() async throws {
        let raw = "Subject: hi\r\n\r\nbody text\r\n"
        let (s, script) = session([
            #"* 1 FETCH (UID 9 BODY[] {\#(raw.utf8.count)}"#,
            raw + ")",
            "a1 OK done",
        ])
        let got = try await s.fetchRaw(uid: 9)
        #expect(String(decoding: got, as: UTF8.self) == raw)
        #expect(await script.written.first?.contains("BODY.PEEK[]") == true)
    }

    /// A refusal is a refusal, and it carries what the server said.
    @Test func aNoReplyBecomesAFailureCarryingItsReason() async throws {
        let (s, _) = session(["a1 NO [AUTHENTICATIONFAILED] Invalid credentials"])
        await #expect(throws: IMAPSession.Failure.self) {
            try await s.login(user: "me", password: "wrong")
        }
    }

    /// A password with a quote in it is quoted rather than sent raw.
    /// Generated app passwords contain `"` and `\` often enough that an
    /// unquoted LOGIN turns one into a syntax error — and the person is
    /// told their password is wrong when it is right.
    @Test func aPasswordWithAQuoteIsEscaped() async throws {
        let (s, script) = session(["a1 OK signed in"])
        try await s.login(user: "me", password: #"pa"ss\word"#)
        let line = await script.written.first ?? ""
        #expect(line.contains(#"\""#))
        #expect(line.contains(#"\\"#))
    }
}
