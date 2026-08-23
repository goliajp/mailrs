import Foundation

/// A POP3 conversation.
///
/// POP3 is not a smaller IMAP; it is a different arrangement, and two
/// differences decide everything this session does:
///
/// - **There are no folders.** A POP3 account has one mailbox, and
///   anything a person filed elsewhere is not visible here at all.
/// - **There are no server-side flags and no stable numbers.** Message
///   3 today is a different message tomorrow, so `UIDL` is the only
///   durable identity, and read state can only be kept on this device.
actor POP3Session {
    enum Failure: Error, Equatable {
        case unreachable(String)
        /// The credential was refused.
        case refused(String)
        /// The server said no to something else.
        case server(String)
        case closed
    }

    private let transport: ByteTransport
    private var buffer = Data()

    init(host: String, port: UInt16) {
        transport = TLSTransport(host: host, port: port)
    }

    /// For tests: a scripted server, and the conversation under test.
    init(transport: ByteTransport) {
        self.transport = transport
    }

    /// Open the connection and read the greeting.
    func connect() async throws {
        do {
            try await transport.connect()
        } catch let e as TransportFailure {
            throw failure(e)
        }
        try expect(await readReply())
    }

    func close() async { await transport.close() }

    /// Sign in.
    ///
    /// `USER` then `PASS`, and **both** answers matter: a server that
    /// accepts the name and refuses the password says so only on the
    /// second, and a client that checks one of them signs in to
    /// nothing.
    func login(user: String, password: String) async throws {
        let named = try await command("USER \(user)")
        if !named.ok { throw refusal(named.text) }
        let secret = try await command("PASS \(password)")
        if !secret.ok { throw refusal(secret.text) }
    }

    /// Every message in the mailbox, by its durable identity.
    func uidls() async throws -> [POP3.Uidl] {
        let reply = try await command("UIDL")
        guard reply.ok else { throw Failure.server(reply.text) }
        var out: [POP3.Uidl] = []
        while true {
            let line = try await readLine()
            if line.trimmingCharacters(in: .whitespaces) == "." { return out }
            if let one = POP3.uidl(line) { out.append(one) }
        }
    }

    /// The headers of one message, without downloading it.
    ///
    /// `TOP n 0` — the headers and none of the body. A list that
    /// fetched whole messages would download the mailbox to show a
    /// list, and on a phone that is somebody's data allowance.
    func headers(number: Int) async throws -> String {
        let reply = try await command("TOP \(number) 0")
        guard reply.ok else { throw Failure.server(reply.text) }
        // Text, here: a header block is what the caller wants, and it
        // is the one part of a message that is safe to read as UTF-8
        // because its non-ASCII arrives as encoded words.
        return SocketText.utf8(POP3.unstuffed(try await readUntilDot()))
    }

    /// One whole message.
    func retrieve(number: Int) async throws -> Data {
        let reply = try await command("RETR \(number)")
        guard reply.ok else { throw Failure.server(reply.text) }
        // Back to the exact bytes. Lossless because every line came
        // out of `SocketText.latin1`, which maps each byte to the code point
        // of the same value: the message says what its own charset is,
        // and nothing here may decide that for it.
        return SocketText.bytes(POP3.unstuffed(try await readUntilDot()))
    }

    /// End the session properly.
    ///
    /// `QUIT` is not politeness: a POP3 server holds an exclusive lock
    /// on the mailbox for the length of a session, and one dropped
    /// without QUIT keeps that lock until it times out — during which
    /// nothing else, including the person's other device, can read
    /// their mail.
    func quit() async {
        _ = try? await command("QUIT")
    }

    // MARK: - the wire

    private func readUntilDot() async throws -> [String] {
        var out: [String] = []
        while true {
            let line = try await readLine()
            out.append(line)
            if line.trimmingCharacters(in: .whitespaces) == "." { return out }
        }
    }

    private func refusal(_ text: String) -> Failure {
        if POP3.isAuthenticationFailure(text) { return .refused(text) }
        return .server(text)
    }

    private func expect(_ reply: POP3.Reply) throws {
        guard reply.ok else { throw Failure.server(reply.text) }
    }

    private func command(_ text: String) async throws -> POP3.Reply {
        try await send(text + "\r\n")
        return try await readReply()
    }

    private func readReply() async throws -> POP3.Reply {
        guard let reply = POP3.reply(try await readLine()) else {
            throw Failure.server("the server did not answer in POP3")
        }
        return reply
    }

    private func send(_ text: String) async throws {
        do {
            try await transport.send(Data(text.utf8))
        } catch let e as TransportFailure {
            throw failure(e)
        }
    }

    private func readLine() async throws -> String {
        while true {
            if let r = buffer.firstRange(of: Data("\r\n".utf8)) {
                // Latin-1 rather than UTF-8: a retrieved message's
                // bytes pass through here, and decoding them as text
                // would settle a charset the message has not been read
                // for yet. Latin-1 is reversible; UTF-8 is not.
                let line = SocketText.latin1(Data(buffer[..<r.lowerBound]))
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

    private func failure(_ e: TransportFailure) -> Failure {
        switch e {
        case let .unreachable(why): return .unreachable(why)
        case .closed: return .closed
        case .cannotUpgrade: return .unreachable("this connection could not be encrypted")
        }
    }
}
