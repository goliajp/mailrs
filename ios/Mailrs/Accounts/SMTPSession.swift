import Foundation
import Network

/// A conversation with a submission server.
///
/// Submission, not delivery: this speaks to the provider's own server
/// on 465 or 587 with the account's credential. Sending mail from a
/// phone straight to the recipient's MX fails SPF and DMARC at every
/// receiver, so it is not an option and not a preference.
actor SMTPSession {
    enum Failure: Error, Equatable {
        case unreachable(String)
        /// The credential was refused — a button to press.
        case refused(String)
        /// The server said no. `permanent` decides whether trying
        /// again could ever work.
        case rejected(code: Int, text: String, permanent: Bool)
        case closed
    }

    private let transport: ByteTransport
    private let host: String
    private var buffer = Data()
    /// Whether TLS was there from the first byte.
    private let implicitTLS: Bool

    /// `port == 465` is TLS from the first byte; 587 is STARTTLS.
    ///
    /// Both are used in the wild and providers disagree about which
    /// they offer, so the port decides rather than a setting nobody
    /// can answer. Outlook and iCloud are 587-only and the provider
    /// table hands out that port, so a session that cannot do the
    /// second is a mailbox that can never send.
    init(host: String, port: UInt16) {
        self.host = host
        implicitTLS = port == 465
        if port == 465 {
            transport = TLSTransport(host: host, port: port)
        } else {
            transport = UpgradableTransport(host: host, port: port)
        }
    }

    /// For tests: a scripted server, and the conversation under test.
    init(host: String, port: UInt16, transport: ByteTransport) {
        self.host = host
        self.transport = transport
        implicitTLS = port == 465
    }

    func connect(helo: String) async throws {
        do {
            try await transport.connect()
        } catch let e as TransportFailure {
            throw failure(e)
        }
        _ = try await readReply()  // the greeting
        let greeted = try await command("EHLO \(helo)")
        guard !implicitTLS else { return }

        // **No downgrade.** A server that does not offer to encrypt is
        // not argued with — the credential simply does not go there. A
        // stripped capability list is what somebody in the middle
        // produces, and refusing is the only answer to it.
        guard greeted.text.uppercased().contains("STARTTLS") else {
            throw Failure.refused("the server did not offer to encrypt the connection")
        }
        let upgrade = try await command("STARTTLS")
        guard upgrade.code == 220 else {
            throw Failure.refused("the server refused to encrypt the connection")
        }
        do {
            try await transport.upgradeToTLS()
        } catch let e as TransportFailure {
            throw failure(e)
        }
        // Again, because everything the server said before the upgrade
        // was said by whoever was on the wire at the time.
        _ = try await command("EHLO \(helo)")
    }

    private func failure(_ e: TransportFailure) -> Failure {
        switch e {
        case let .unreachable(why): return .unreachable(why)
        case .closed: return .closed
        case .cannotUpgrade: return .refused("this connection could not be encrypted")
        }
    }

    func close() async { await transport.close() }

    /// Sign in with a password or an access token.
    ///
    /// An access token is not a password: it goes through XOAUTH2, and
    /// sending one through `AUTH PLAIN` is refused — after which the
    /// person is told their password is wrong for an account whose
    /// credentials are fine.
    func authenticate(user: String, secret: String, oauth: Bool) async throws {
        var reply: SMTP.Reply
        if oauth {
            reply = try await command("AUTH XOAUTH2 \(SMTP.authXOAuth2(user: user, token: secret))")
        } else {
            reply = try await command("AUTH PLAIN \(SMTP.authPlain(user: user, password: secret))")
        }
        // **334 first.** It is inside the range `isPositive` calls
        // success, so testing that first made this branch unreachable
        // and reported every refused token as signed in — which is
        // exactly what the note below says must not happen. A provider
        // only sends 334 when a token is genuinely bad, so a real
        // server never shows it; a scripted one does.
        //
        // A provider that rejects an OAuth token answers 334 with a
        // base64 error rather than a final code, and waits for an
        // empty line before sending one.
        if reply.code == 334 {
            let final = try await command("")
            throw Failure.refused(final.text)
        }
        if reply.isPositive { return }
        if SMTP.isAuthenticationFailure(code: reply.code, text: reply.text) {
            throw Failure.refused(reply.text)
        }
        throw Failure.rejected(code: reply.code, text: reply.text, permanent: reply.isPermanent)
    }

    /// Hand one message over.
    func send(from: String, to: [String], message: String) async throws {
        try await send(from: from, to: to, message: AnySequence([message]))
    }

    /// Hand one message over, in as many pieces as it comes in.
    ///
    /// **Streamed rather than assembled.** A 25 MB attachment built
    /// into one string and dot-stuffed into another is several times
    /// its own size in memory at once, and on a phone that is a
    /// process the system kills — which looks exactly like mail that
    /// vanished. Here each piece is stuffed and written as it arrives,
    /// so what is held is one piece.
    ///
    /// The stuffing is `DotStuffer`'s, not `SMTP.dotStuffed`'s,
    /// because a piece can end on the line break whose next line
    /// begins with a dot — and a stuffer with no memory of that
    /// truncates the message there while it still arrives looking
    /// complete.
    func send(from: String, to: [String], message: AnySequence<String>) async throws {
        try await expect(try await command("MAIL FROM:<\(from)>"))
        for rcpt in to {
            try await expect(try await command("RCPT TO:<\(rcpt)>"))
        }
        let start = try await command("DATA")
        guard start.code == 354 else {
            throw Failure.rejected(
                code: start.code, text: start.text, permanent: start.isPermanent)
        }
        let stuffer = DotStuffer()
        var last = ""
        for piece in message where !piece.isEmpty {
            try await write(stuffer.feed(piece))
            last = piece
        }
        // The terminator needs a line of its own, and a message that
        // already ended on one must not gain a blank line — some
        // servers keep it and the reader sees it.
        try await write(last.hasSuffix("\r\n") ? ".\r\n" : "\r\n.\r\n")
        try await expect(try await readReply())
        _ = try? await command("QUIT")
    }

    // MARK: - the wire

    private func expect(_ reply: SMTP.Reply) throws {
        guard reply.isPositive else {
            throw Failure.rejected(
                code: reply.code, text: reply.text, permanent: reply.isPermanent)
        }
    }

    private func command(_ text: String) async throws -> SMTP.Reply {
        try await write(text + "\r\n")
        return try await readReply()
    }

    /// A reply, however many lines it takes.
    ///
    /// `250-STARTTLS` continues and `250 OK` ends; reading only the
    /// first line leaves the rest in the buffer, and the next command
    /// then reads somebody else's answer.
    private func readReply() async throws -> SMTP.Reply {
        // **Every line, joined.** A multi-line reply is one reply, and
        // keeping only the last discards the EHLO capability list —
        // the only place `STARTTLS` is announced, and so the only way
        // to know whether an upgrade is possible at all.
        var gathered: [String] = []
        while true {
            let line = try await readLine()
            guard let r = SMTP.reply(line) else { continue }
            gathered.append(r.text)
            if !r.more {
                return SMTP.Reply(
                    code: r.code, text: gathered.joined(separator: "\n"), more: false)
            }
        }
    }

    private func write(_ text: String) async throws {
        do {
            try await transport.send(Data(text.utf8))
        } catch let e as TransportFailure {
            throw failure(e)
        }
    }

    private func readLine() async throws -> String {
        while true {
            if let r = buffer.firstRange(of: Data("\r\n".utf8)) {
                let line = String(decoding: buffer[..<r.lowerBound], as: UTF8.self)
                buffer.removeSubrange(..<r.upperBound)
                return line
            }
            let chunk = try await receive()
            if chunk.isEmpty { throw Failure.closed }
            buffer.append(chunk)
        }
    }

    private func receive() async throws -> Data {
        do {
            return try await transport.receive()
        } catch let e as TransportFailure {
            throw failure(e)
        }
    }
}
