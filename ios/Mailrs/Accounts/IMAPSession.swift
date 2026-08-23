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

    private let connection: NWConnection
    private var tag = 0
    private var buffer = Data()
    /// How long any one command may take before it is a timeout.
    ///
    /// Generous, because a large FETCH on a slow link is not a fault.
    private let commandTimeout: Duration = .seconds(60)

    init(host: String, port: UInt16) {
        let tls = NWProtocolTLS.Options()
        let params = NWParameters(tls: tls, tcp: .init())
        connection = NWConnection(
            host: .init(host),
            port: .init(rawValue: port) ?? 993,
            using: params
        )
    }

    /// Open the connection and read the greeting.
    func connect() async throws {
        // `stateUpdateHandler` fires more than once and from another
        // queue, and a continuation may be resumed exactly once —
        // resuming twice is a crash, not a warning. The box is what
        // makes "first one wins" true across threads.
        let once = ResumeOnce()
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Void, Error>) in
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    once.resume(k, with: .success(()))
                case let .failed(e):
                    once.resume(k, with: .failure(Failure.unreachable(e.localizedDescription)))
                case .cancelled:
                    once.resume(k, with: .failure(Failure.closed))
                default:
                    break
                }
            }
            connection.start(queue: .global(qos: .userInitiated))
        }
        // The greeting. A server that refuses the connection says so
        // here — `* BYE`, often with a reason worth showing.
        let greeting = try await readLine()
        if greeting.uppercased().hasPrefix("* BYE") {
            throw Failure.server(String(greeting.dropFirst(2)))
        }
    }

    func close() {
        connection.cancel()
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
        while buffer.count < count {
            let chunk = try await receive()
            if chunk.isEmpty { throw Failure.closed }
            buffer.append(chunk)
        }
        let body = buffer.prefix(count)
        buffer.removeFirst(count)
        return String(decoding: body, as: UTF8.self)
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
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Data, Error>) in
            connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) {
                data, _, isComplete, error in
                if let error {
                    k.resume(throwing: Failure.unreachable(error.localizedDescription))
                    return
                }
                if let data, !data.isEmpty {
                    k.resume(returning: data)
                    return
                }
                k.resume(returning: isComplete ? Data() : Data())
            }
        }
    }
}
