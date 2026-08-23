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

    private let connection: NWConnection
    private let host: String
    private var buffer = Data()
    /// Whether TLS was there from the first byte.
    private let implicitTLS: Bool

    /// `port == 465` is TLS from the first byte; 587 is STARTTLS.
    ///
    /// Both are used in the wild and providers disagree about which
    /// they offer, so the port decides rather than a setting nobody
    /// can answer.
    init(host: String, port: UInt16) {
        self.host = host
        implicitTLS = port == 465
        let params: NWParameters =
            port == 465
            ? NWParameters(tls: NWProtocolTLS.Options(), tcp: .init())
            : NWParameters(tls: nil, tcp: .init())
        connection = NWConnection(
            host: .init(host), port: .init(rawValue: port) ?? 465, using: params)
    }

    func connect(helo: String) async throws {
        let once = ResumeOnce()
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Void, Error>) in
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready: once.resume(k, with: .success(()))
                case let .failed(e):
                    once.resume(k, with: .failure(Failure.unreachable(e.localizedDescription)))
                case .cancelled: once.resume(k, with: .failure(Failure.closed))
                default: break
                }
            }
            connection.start(queue: .global(qos: .userInitiated))
        }
        _ = try await readReply()  // the greeting
        _ = try await command("EHLO \(helo)")
        if !implicitTLS {
            // **No downgrade.** A server that will not start TLS is a
            // server this credential does not go to — sending it in
            // the clear hands it to anything on the path.
            let r = try await command("STARTTLS")
            guard r.isPositive else {
                throw Failure.rejected(
                    code: r.code, text: "this server refused STARTTLS", permanent: true)
            }
            throw Failure.rejected(
                code: 0,
                text: "STARTTLS is not supported yet — use port 465",
                permanent: true)
        }
    }

    func close() { connection.cancel() }

    /// Sign in with a password or an access token.
    ///
    /// An access token is not a password: it goes through XOAUTH2, and
    /// sending one through `AUTH PLAIN` is refused — after which the
    /// person is told their password is wrong for an account whose
    /// credentials are fine.
    func authenticate(user: String, secret: String, oauth: Bool) async throws {
        let reply =
            oauth
            ? try await command("AUTH XOAUTH2 \(SMTP.authXOAuth2(user: user, token: secret))")
            : try await command("AUTH PLAIN \(SMTP.authPlain(user: user, password: secret))")
        if reply.isPositive { return }
        // A provider that rejects an OAuth token answers 334 with a
        // base64 error rather than a final code, and waits for an
        // empty line before sending one. Reading the 334 as success
        // authenticates every refused token.
        if reply.code == 334 {
            let final = try await command("")
            throw Failure.refused(final.text)
        }
        throw SMTP.isAuthenticationFailure(code: reply.code, text: reply.text)
            ? Failure.refused(reply.text)
            : Failure.rejected(code: reply.code, text: reply.text, permanent: reply.isPermanent)
    }

    /// Hand one message over.
    func send(from: String, to: [String], message: String) async throws {
        try await expect(try await command("MAIL FROM:<\(from)>"))
        for rcpt in to {
            try await expect(try await command("RCPT TO:<\(rcpt)>"))
        }
        let start = try await command("DATA")
        guard start.code == 354 else {
            throw Failure.rejected(
                code: start.code, text: start.text, permanent: start.isPermanent)
        }
        // Dot-stuffed: a body line beginning with `.` would end the
        // block here and the message would arrive cut in half.
        try await write(SMTP.dotStuffed(message) + "\r\n.\r\n")
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
        while true {
            let line = try await readLine()
            guard let r = SMTP.reply(line) else { continue }
            if !r.more { return r }
        }
    }

    private func write(_ text: String) async throws {
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Void, Error>) in
            connection.send(
                content: Data(text.utf8),
                completion: .contentProcessed { error in
                    if let error {
                        k.resume(throwing: Failure.unreachable(error.localizedDescription))
                    } else {
                        k.resume()
                    }
                })
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
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Data, Error>) in
            connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) {
                data, _, _, error in
                if let error {
                    k.resume(throwing: Failure.unreachable(error.localizedDescription))
                } else {
                    k.resume(returning: data ?? Data())
                }
            }
        }
    }
}
