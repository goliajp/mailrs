import Foundation
import Testing

@testable import Mailrs

/// A server that says exactly what it is told to say.
actor ScriptedTransport: ByteTransport {
    private var lines: [String]
    private(set) var written: [String] = []
    private(set) var upgraded = false

    init(_ lines: [String]) { self.lines = lines }

    func connect() async throws {}

    func receive() async throws -> Data {
        guard !lines.isEmpty else { throw TransportFailure.closed }
        return Data((lines.removeFirst() + "\r\n").utf8)
    }

    func send(_ data: Data) async throws {
        written.append(String(decoding: data, as: UTF8.self))
    }

    func upgradeToTLS() async throws { upgraded = true }
    func close() {}
}

/// The SMTP conversation, without a server.
///
/// A real server is exactly the thing that will not send the awkward
/// case on demand — a multi-line greeting, a 334 refusal of an OAuth
/// token, a 4xx when the queue is full. A scripted one sends them every
/// time.
@Suite struct SMTPSessionTests {
    private func session(port: UInt16 = 587, _ lines: [String])
        -> (SMTPSession, ScriptedTransport)
    {
        let script = ScriptedTransport(lines)
        return (SMTPSession(host: "localhost", port: port, transport: script), script)
    }

    /// The **fourth character** says whether more is coming — `250-` is
    /// a continuation and `250 ` is the end. A parser that reads the
    /// code alone stops at the first line of every EHLO and then reads
    /// the capability list as the answer to the next command.
    @Test func aMultiLineReplyIsReadToItsEnd() async throws {
        let (s, script) = session([
            "220 smtp.example.com ESMTP",
            "250-smtp.example.com greets you",
            "250-SIZE 35882577",
            "250-AUTH LOGIN PLAIN XOAUTH2",
            "250 STARTTLS",
            "220 2.0.0 Ready to start TLS",
            "250-smtp.example.com greets you again",
            "250 STARTTLS",
            "250 2.1.0 sender ok",
            "250 2.1.5 recipient ok",
            "354 go ahead",
            "250 2.0.0 queued",
            "221 bye",
        ])
        try await s.connect(helo: "example.com")
        // If EHLO had been read short, this MAIL FROM would consume one
        // of the capability lines instead of its own reply, and the
        // whole exchange would slide by one.
        try await s.send(
            from: "me@example.com", to: ["you@example.com"],
            message: "Subject: x\r\n\r\nhi\r\n")
        let sent = await script.written
        #expect(sent.contains { $0.hasPrefix("EHLO example.com") })
        #expect(sent.contains { $0.hasPrefix("MAIL FROM:<me@example.com>") })
        #expect(await script.upgraded)
    }

    /// `AUTH PLAIN` sends an empty authorisation identity, then the
    /// login, then the secret, \u{0}-separated. Putting the login in the
    /// first field as well — the obvious misreading — is refused by
    /// some servers and silently accepted as a different user by
    /// others.
    @Test func authPlainSeparatesWithNulAndLeadsWithNothing() async throws {
        let (s, script) = session(["235 2.7.0 Accepted"])
        try await s.authenticate(user: "me@example.com", secret: "secret", oauth: false)
        let line = await script.written.first { $0.hasPrefix("AUTH PLAIN") } ?? ""
        let payload = line.replacingOccurrences(of: "AUTH PLAIN ", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let decoded = String(decoding: Data(base64Encoded: payload) ?? Data(), as: UTF8.self)
        #expect(decoded == "\u{0}me@example.com\u{0}secret")
    }

    /// A provider that rejects an OAuth token answers **334** with a
    /// base64 error rather than a final code. 334 is inside the range
    /// `isPositive` calls success, so a session that tests that first
    /// reports every refused token as signed in.
    @Test func a334IsARefusalAndNotASuccess() async throws {
        let (s, _) = session([
            "334 eyJzdGF0dXMiOiI0MDEifQ==",
            "535 5.7.8 Username and Password not accepted",
        ])
        await #expect(throws: SMTPSession.Failure.self) {
            try await s.authenticate(user: "me@example.com", secret: "token", oauth: true)
        }
    }

    /// A body line beginning with `.` would end the DATA block. Left
    /// unstuffed, the message arrives cut in half — and the half that
    /// arrives looks like a whole message.
    @Test func aBodyLineOfASingleDotIsStuffed() async throws {
        let (s, script) = session(
            port: 465,
            [
                "250 2.1.0 sender ok", "250 2.1.5 recipient ok", "354 go ahead",
                "250 2.0.0 queued", "221 bye",
            ])
        try await s.send(
            from: "me@a.com", to: ["you@b.com"],
            message: "Subject: x\r\n\r\n.\r\nnot the end\r\n")
        let data = await script.written.first { $0.contains("not the end") } ?? ""
        #expect(data.contains("\r\n..\r\n"))
        #expect(data.hasSuffix("\r\n.\r\n"), "the block was never terminated")
    }

    /// **No downgrade.** A server that does not offer to encrypt is not
    /// argued with — the credential simply does not go there. A
    /// stripped capability list is what somebody in the middle
    /// produces, and refusing is the only answer to it.
    @Test func aServerThatWillNotEncryptGetsNoCredential() async throws {
        let (s, script) = session([
            "220 smtp.example.com ESMTP",
            "250-smtp.example.com greets you",
            "250 SIZE 35882577",
        ])
        await #expect(throws: SMTPSession.Failure.self) {
            try await s.connect(helo: "example.com")
        }
        #expect(await !script.upgraded)
        #expect(await !script.written.contains { $0.hasPrefix("STARTTLS") })
    }

    /// And one that offers it and then refuses is dropped too.
    @Test func aRefusedUpgradeIsNotContinuedInTheClear() async throws {
        let (s, script) = session([
            "220 smtp.example.com ESMTP",
            "250-smtp.example.com greets you",
            "250 STARTTLS",
            "454 4.7.0 TLS not available",
        ])
        await #expect(throws: SMTPSession.Failure.self) {
            try await s.connect(helo: "example.com")
        }
        #expect(await !script.upgraded)
    }

    /// 465 is encrypted from the first byte, so it neither offers nor
    /// needs STARTTLS — asking for it there is a command the server has
    /// every right to refuse.
    @Test func implicitTlsDoesNotAskToUpgrade() async throws {
        let (s, script) = session(port: 465, ["220 ready", "250 smtp.example.com"])
        try await s.connect(helo: "example.com")
        #expect(await !script.written.contains { $0.hasPrefix("STARTTLS") })
        #expect(await !script.upgraded)
    }

    /// Every recipient is offered, and one refusal is not silent.
    @Test func eachRecipientIsNamed() async throws {
        let (s, script) = session(port: 465, ["250 ok", "250 ok", "550 no such user"])
        await #expect(throws: SMTPSession.Failure.self) {
            try await s.send(from: "me@a.com", to: ["a@b.com", "c@d.com"], message: "x")
        }
        let sent = await script.written
        #expect(sent.contains { $0.hasPrefix("RCPT TO:<a@b.com>") })
        #expect(sent.contains { $0.hasPrefix("RCPT TO:<c@d.com>") })
    }
}
