import Foundation
import Network

/// A conversation with an IMAP server.
///
/// The socket half. Everything it reads is handed to `IMAP` to
/// interpret, so this file is about bytes and timeouts and that file
/// is about grammar — and only one of them needs a server to test.
///
/// **TLS from the first byte.** No plaintext, and no STARTTLS: a
/// credential is not sent over a connection that was ever in the
/// clear, and every provider worth connecting to offers 993.
actor IMAPSession {
    enum Failure: Error, Equatable {
        /// Could not reach the server at all.
        case unreachable(String)
        /// The credential was refused. **Not a network problem** — one
        /// is a button to press, the other is waiting.
        case refused(String)
        /// The server said no to something else.
        case server(String)
        /// The connection broke mid-conversation.
        case closed
        /// The server did not answer in time.
        case timedOut
    }

    private let transport: ByteTransport
    private var tag = 0
    private var buffer = Data()
    /// How long any one command may take before it is a timeout.
    ///
    /// Generous, because a large FETCH on a slow link is not a fault.
    private let commandTimeout: Duration = .seconds(60)

    init(host: String, port: UInt16) {
        transport = TLSTransport(host: host, port: port)
    }

    /// For tests: a scripted server, and the conversation under test.
    ///
    /// Every rule this session enforces — a tag that must not be
    /// matched by its own prefix, a literal read by the byte count the
    /// server announced, a folder name at the end of a LIST line — is
    /// a rule about what arrives, and a real server will not send the
    /// awkward case on demand.
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
        // The greeting. A server that refuses the connection says so
        // here — `* BYE`, often with a reason worth showing.
        let greeting = try await readLine()
        if greeting.uppercased().hasPrefix("* BYE") {
            throw Failure.server(String(greeting.dropFirst(2)))
        }
    }

    func close() async {
        await transport.close()
    }

    private func failure(_ e: TransportFailure) -> Failure {
        switch e {
        case let .unreachable(why): return .unreachable(why)
        case .closed: return .closed
        case .cannotUpgrade: return .unreachable("this connection could not be encrypted")
        }
    }

    /// Sign in.
    ///
    /// The password is quoted: generated app passwords contain `"` and
    /// `\` often enough that an unquoted LOGIN turns one into a syntax
    /// error — and the person is told their password is wrong when it
    /// is right.
    func login(user: String, password: String) async throws {
        let (_, completion) = try await command(
            "LOGIN \(IMAP.quoted(user)) \(IMAP.quoted(password))")
        switch completion {
        case .ok:
            return
        case let .no(detail):
            throw IMAP.isAuthenticationFailure(detail)
                ? Failure.refused(detail) : Failure.server(detail)
        case let .bad(detail):
            throw Failure.server(detail)
        }
    }

    /// Sign in with an access token.
    ///
    /// An access token is not a password: `\u{1}` separators, an
    /// `auth=Bearer ` prefix, two terminators — and a different
    /// failure protocol. A provider that rejects the token answers a
    /// continuation (`+`) with a base64 error rather than a final
    /// code, and will not send one until the client sends an empty
    /// line. Reading that `+` as success authenticates every refused
    /// token.
    func authenticateXOAuth2(user: String, token: String) async throws {
        let raw = "user=\(user)\u{1}auth=Bearer \(token)\u{1}\u{1}"
        let payload = Data(raw.utf8).base64EncodedString()
        let t = nextTag()
        try await send("\(t) AUTHENTICATE XOAUTH2 \(payload)\r\n")
        while true {
            let line = try await readLine()
            if line.hasPrefix("+") {
                // Refused. Answer the continuation so the server sends
                // the tagged failure rather than waiting for us.
                try await send("\r\n")
                continue
            }
            if let completion = IMAP.completion(of: line, tag: t) {
                switch completion {
                case .ok: return
                case let .no(d): throw Failure.refused(d)
                case let .bad(d): throw Failure.server(d)
                }
            }
        }
    }

    /// Every folder the server offers.
    func list() async throws -> [(name: String, attributes: [String])] {
        let (untagged, completion) = try await command(#"LIST "" "*""#)
        if case let .no(d) = completion { throw Failure.server(d) }
        if case let .bad(d) = completion { throw Failure.server(d) }
        return untagged.compactMap {
            if case let .list(name, attributes) = $0 { return (name, attributes) }
            return nil
        }
    }

    /// Open a folder, and say what state it is in.
    func select(_ folder: String) async throws -> (uidValidity: UInt32, exists: Int) {
        let (untagged, completion) = try await command("SELECT \(IMAP.quoted(folder))")
        if case let .no(d) = completion { throw Failure.server(d) }
        if case let .bad(d) = completion { throw Failure.server(d) }
        var validity: UInt32 = 0
        var exists = 0
        for u in untagged {
            if case let .uidValidity(v) = u { validity = v }
            if case let .exists(n) = u { exists = n }
        }
        return (validity, exists)
    }

    /// One message, as far as a list row needs it.
    struct Fetched: Equatable {
        let uid: UInt32
        let seen: Bool
        let headers: MessageHeaders.Parsed
        /// Seconds since the epoch, or nil when the `Date:` header
        /// could not be read. **Nil, not now** — a message shown as
        /// having just arrived jumps to the top and stays there.
        let date: Int64?
    }

    /// Read the headers of everything in `range`.
    ///
    /// `BODY.PEEK[HEADER]`, not `BODY[HEADER]`: the second marks every
    /// message read on the server just for having been listed, which
    /// is the rudest thing a mail client can do to somebody's mailbox.
    ///
    /// The literal is read **by the byte count the server announced**,
    /// never by scanning for a terminator: a message contains every
    /// byte sequence a terminator could be made of.
    func fetchHeaders(range: String) async throws -> [Fetched] {
        let t = nextTag()
        try await send("\(t) UID FETCH \(range) (UID FLAGS BODY.PEEK[HEADER])\r\n")
        var out: [Fetched] = []
        while true {
            let line = try await readLine()
            if let done = IMAP.completion(of: line, tag: t) {
                switch done {
                case .ok: return out
                case let .no(d): throw Failure.server(d)
                case let .bad(d): throw Failure.server(d)
                }
            }
            guard let announced = IMAP.fetchLine(line), let uid = announced.uid else { continue }
            guard let count = announced.literalBytes else {
                // A flags-only reply: nothing to read, and nothing a
                // row can show that it does not already have.
                continue
            }
            let raw = try await readBytes(count)
            let headers = MessageHeaders.parse(raw)
            out.append(
                Fetched(
                    uid: uid,
                    seen: announced.seen,
                    headers: headers,
                    date: MailDate.epochSeconds(headers.date)))
        }
    }

    /// Exactly `count` bytes, whatever they contain.
    private func readBytes(_ count: Int) async throws -> String {
        String(decoding: try await readRaw(count), as: UTF8.self)
    }

    /// Exactly `count` bytes, undecoded.
    ///
    /// A body is not text until something has read the charset it
    /// declares, and that header is inside the bytes. Decoding here
    /// would settle the question before it has been asked.
    private func readRaw(_ count: Int) async throws -> Data {
        while buffer.count < count {
            let chunk = try await receive()
            if chunk.isEmpty { throw Failure.closed }
            buffer.append(chunk)
        }
        let body = buffer.prefix(count)
        buffer.removeFirst(count)
        return Data(body)
    }

    /// One message, whole.
    ///
    /// `BODY.PEEK[]` rather than `BODY[]` for the same reason the
    /// header fetch peeks: opening a message is the reader's decision,
    /// and a client that marks mail read for having looked at it takes
    /// that decision away. Marking read is a separate, deliberate call.
    func fetchRaw(uid: UInt32) async throws -> Data {
        let t = nextTag()
        try await send("\(t) UID FETCH \(uid) (BODY.PEEK[])\r\n")
        var out = Data()
        while true {
            let line = try await readLine()
            if let done = IMAP.completion(of: line, tag: t) {
                switch done {
                case .ok: return out
                case let .no(d): throw Failure.server(d)
                case let .bad(d): throw Failure.server(d)
                }
            }
            guard let announced = IMAP.fetchLine(line), let count = announced.literalBytes
            else { continue }
            // The announced byte count, never a scan for a terminator:
            // a message contains every byte sequence a terminator could
            // be made of.
            out = try await readRaw(count)
        }
    }

    /// Mark a message read on the server, because somebody read it.
    func markSeen(uid: UInt32) async throws {
        try await store(uid: uid, op: "+FLAGS", flag: "\\Seen")
    }

    /// Mark it unread again.
    ///
    /// `-FLAGS`, not a `FLAGS` that names what should remain: the
    /// second replaces the whole set, so it would quietly clear
    /// `\Flagged`, `\Answered` and every keyword the person or another
    /// client had put there.
    func markUnseen(uid: UInt32) async throws {
        try await store(uid: uid, op: "-FLAGS", flag: "\\Seen")
    }

    private func store(uid: UInt32, op: String, flag: String) async throws {
        let t = nextTag()
        try await send("\(t) UID STORE \(uid) \(op) (\(flag))\r\n")
        try await awaitCompletion(t)
    }

    /// What the server said it can do.
    ///
    /// Read rather than assumed: the two commands below exist only on
    /// some servers, and asking for one that is not there is an error
    /// the person sees rather than a fallback they do not.
    func capabilities() async throws -> Set<String> {
        let (untagged, done) = try await command("CAPABILITY")
        if case let .no(d) = done { throw Failure.server(d) }
        if case let .bad(d) = done { throw Failure.server(d) }
        var out: Set<String> = []
        for line in untagged {
            if case let .capabilities(names) = line {
                for name in names { out.insert(name.uppercased()) }
            }
        }
        return out
    }

    /// Put a message in another folder.
    ///
    /// `MOVE` (RFC 6851) where the server has it, and the older
    /// three-step dance where it does not — and the difference matters
    /// more than it looks:
    ///
    /// **A bare `EXPUNGE` removes every message in the folder flagged
    /// `\Deleted`**, including ones somebody else's client flagged and
    /// has not expunged yet. `UID EXPUNGE` (RFC 4315) removes only the
    /// one named. Where neither `MOVE` nor `UIDPLUS` is offered, the
    /// message is flagged and **left** rather than expunged: it
    /// disappears from the list either way, and no other message is
    /// taken with it.
    func moveTo(uid: UInt32, folder: String, capabilities: Set<String>) async throws {
        if capabilities.contains("MOVE") {
            let t = nextTag()
            try await send("\(t) UID MOVE \(uid) \(IMAP.quoted(folder))\r\n")
            try await awaitCompletion(t)
            return
        }
        let copy = nextTag()
        try await send("\(copy) UID COPY \(uid) \(IMAP.quoted(folder))\r\n")
        try await awaitCompletion(copy)
        try await store(uid: uid, op: "+FLAGS", flag: "\\Deleted")
        if capabilities.contains("UIDPLUS") {
            let expunge = nextTag()
            try await send("\(expunge) UID EXPUNGE \(uid)\r\n")
            try await awaitCompletion(expunge)
        }
    }

    private func awaitCompletion(_ tag: String) async throws {
        while true {
            let line = try await readLine()
            if let done = IMAP.completion(of: line, tag: tag) {
                switch done {
                case .ok: return
                case let .no(d): throw Failure.server(d)
                case let .bad(d): throw Failure.server(d)
                }
            }
        }
    }

    // MARK: - the wire

    private func nextTag() -> String {
        tag += 1
        return "a\(tag)"
    }

    /// Send a command and read until its tagged reply.
    private func command(_ text: String) async throws -> ([IMAP.Untagged], IMAP.Completion) {
        let t = nextTag()
        try await send("\(t) \(text)\r\n")
        var untagged: [IMAP.Untagged] = []
        while true {
            let line = try await readLine()
            if let done = IMAP.completion(of: line, tag: t) {
                return (untagged, done)
            }
            if let u = IMAP.untagged(line) {
                untagged.append(u)
            }
        }
    }

    private func send(_ text: String) async throws {
        do {
            try await transport.send(Data(text.utf8))
        } catch let e as TransportFailure {
            throw failure(e)
        }
    }

    /// One CRLF-terminated line.
    ///
    /// Buffered, because a read returns whatever arrived rather than
    /// one line: two replies can land in a single packet, and half a
    /// reply can land in two.
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
