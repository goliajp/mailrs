import Foundation

/// Sending through a connected account.
///
/// What goes on the wire is `OutgoingMessage`, which is pure and
/// tested. This opens the socket and reports what came back in words a
/// person can act on — a rejection from a mail server is a number and
/// a sentence written for another machine.
enum AccountSender {
    /// How a session is made.
    ///
    /// Injectable so the builder and the wire can be checked together.
    /// Both halves are tested apart, and the seam between them is
    /// where **a Bcc would leak** — the address belongs in `RCPT TO`
    /// and nowhere in the DATA block, and only an end-to-end look can
    /// say that it is so.
    nonisolated(unsafe) static var openSmtp: (String, UInt16) -> SMTPSession = {
        SMTPSession(host: $0, port: $1)
    }
    enum Outcome: Equatable {
        case sent
        case failed(String)
    }

    static func send(_ draft: OutgoingMessage.Draft, from account: MailAccount, bcc: [String] = [])
        async -> Outcome
    {
        let recipients = OutgoingMessage.envelope(draft, bcc: bcc)
        guard !recipients.isEmpty else { return .failed("Add somebody to send this to") }
        // Refused before sending rather than discovered during it: a
        // message stopped here is a message somebody still has, and one
        // that dies mid-send looks exactly like mail that vanished.
        if case let .tooLarge(attached, limit) = OutgoingLimits.check(draft) {
            let attachedText = attached.formatted(.byteCount(style: .file))
            let limitText = limit.formatted(.byteCount(style: .file))
            return .failed(
                "Too large to send: \(attachedText) attached, and about \(limitText) is the most.")
        }
        guard let secret = AccountStore.secret(for: account.id) else {
            return .failed("Sign in again to send from this account")
        }
        // Streamed, not assembled: the socket pulls 57 bytes of a file
        // at a time, so a large attachment is never in memory whole.
        let message = OutgoingMessage.pieces(
            draft, id: identity(for: account), date: Date())
        let session = openSmtp(account.smtpHost, account.smtpPort)
        do {
            // The domain of the address, not the device's name: a HELO
            // naming somebody's phone is refused by a fair number of
            // servers and greylisted by more.
            try await session.connect(helo: helo(for: account))
            try await session.authenticate(
                user: account.loginName, secret: secret, oauth: account.auth == .oauth2)
            // The envelope sender is the account's own address. A
            // server that permits one address will refuse another, and
            // SPF makes that refusal correct.
            try await session.send(from: account.address, to: recipients, message: message)
            await session.close()
            return .sent
        } catch let e as SMTPSession.Failure {
            await session.close()
            return .failed(explain(e))
        } catch {
            await session.close()
            return .failed("Could not reach the outgoing server")
        }
    }

    /// A Message-ID nobody else will mint.
    ///
    /// The domain half is the account's own, because a Message-ID
    /// pointing at a domain that has nothing to do with the sender is
    /// one of the things spam filters count.
    static func identity(for account: MailAccount, uuid: String = UUID().uuidString) -> String {
        "\(uuid.lowercased())@\(domain(of: account.address))"
    }

    static func helo(for account: MailAccount) -> String {
        let host = domain(of: account.address)
        if host.isEmpty { return "localhost" }
        return host
    }

    private static func domain(of address: String) -> String {
        let parts = address.split(separator: "@")
        guard parts.count == 2 else { return "" }
        return String(parts[1]).lowercased()
    }

    /// A server's refusal, in words somebody can act on.
    static func explain(_ e: SMTPSession.Failure) -> String {
        switch e {
        case let .rejected(code, text, permanent):
            // 5xx is the message's fault and 4xx is the moment's; a
            // person told "try again" about a permanent rejection will
            // try again forever.
            if !permanent { return "The server is busy — try again shortly (\(code))" }
            if code == 550 || code == 553 {
                return "The server refused the recipient or the sender address (\(code)): \(text)"
            }
            if code == 535 { return "The server refused the sign-in for this account (535)" }
            return "The server refused this message (\(code)): \(text)"
        case let .refused(detail): return AccountConnection.readable(detail)
        case let .unreachable(detail): return AccountConnection.readable(detail)
        case .closed: return "The outgoing server closed the connection"
        }
    }
}
