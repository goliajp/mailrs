import Foundation

/// Proving an account works, before anything is stored.
///
/// A credential that is saved and then found to be wrong is an account
/// that sits in the list doing nothing, and the person has no way to
/// tell that from "there is no new mail". Connecting first turns that
/// into a sentence on the screen they were already looking at.
///
/// Both halves are checked. A password that reads mail but cannot send
/// is a real thing — providers hand out separate app passwords, and
/// some accept one for IMAP and refuse it for submission — and finding
/// that out at the moment somebody presses Send is the worst time.
enum AccountConnection {
    /// What went wrong, in words somebody can act on.
    struct Failure: Error, Equatable {
        /// Which half refused.
        let stage: Stage
        /// What to tell the person.
        let message: String
        /// Whether re-typing the secret could fix it.
        let credential: Bool

        enum Stage: String, Equatable {
            case incoming = "incoming server"
            case outgoing = "outgoing server"
        }
    }

    static func verify(_ account: MailAccount, secret: String) async -> Failure? {
        // The kind decides who is asked. Verifying a POP3 account by
        // talking IMAP to it fails for a reason that has nothing to do
        // with the credential — and the person is told their password
        // is wrong.
        if account.incoming == .pop3 { return await verifyPop3(account, secret: secret) }
        if account.incoming == .jmap { return await verifyJmap(account, secret: secret) }
        let imap = IMAPSession(host: account.imapHost, port: account.imapPort)
        do {
            try await imap.connect()
            if account.auth == .oauth2 {
                try await imap.authenticateXOAuth2(user: account.loginName, token: secret)
            } else {
                try await imap.login(user: account.loginName, password: secret)
            }
            // Not just the login: a server may accept a credential and
            // then refuse to list, and an account that cannot list is
            // an account with no folders to read.
            _ = try await imap.list()
            await imap.close()
        } catch let e as IMAPSession.Failure {
            await imap.close()
            return failure(.incoming, e)
        } catch {
            await imap.close()
            return Failure(
                stage: .incoming, message: error.localizedDescription, credential: false)
        }

        return await verifySmtp(account, secret: secret)
    }

    /// A POP3 account, checked as far as it can be.
    ///
    /// The listing as well as the login, for the same reason IMAP's
    /// check lists folders: a server may accept a credential and then
    /// refuse to say what is in the mailbox, and an account that
    /// cannot list is an account with nothing to show.
    ///
    /// And `QUIT`, because the check itself holds the exclusive lock
    /// every POP3 session holds — one left open makes the mailbox
    /// unreadable everywhere else until it times out.
    private static func verifyPop3(_ account: MailAccount, secret: String) async -> Failure? {
        let pop = POP3Session(host: account.imapHost, port: account.imapPort)
        do {
            try await pop.connect()
            try await pop.login(user: account.loginName, password: secret)
            _ = try await pop.uidls()
            await pop.quit()
            await pop.close()
        } catch let e as POP3Session.Failure {
            await pop.close()
            return failure(.incoming, e)
        } catch {
            await pop.close()
            return Failure(
                stage: .incoming, message: error.localizedDescription, credential: false)
        }
        return await verifySmtp(account, secret: secret)
    }

    /// A JMAP account: the session object, then one real request.
    ///
    /// Not just the session — a server will hand out
    /// `/.well-known/jmap` to anybody, so reading it proves nothing
    /// about the credential.
    private static func verifyJmap(_ account: MailAccount, secret: String) async -> Failure? {
        var user = account.loginName
        if account.auth == .oauth2 { user = "" }
        let client = JMAPClient(host: account.imapHost)
        do {
            let session = try await client.session(user: user, secret: secret)
            _ = try await client.newest(session: session, user: user, secret: secret, limit: 1)
            // No SMTP: JMAP submits over the same API, so the account
            // that reads is the account that sends.
            return nil
        } catch let e as JMAPClient.Failure {
            var credential = false
            var message = ""
            switch e {
            case let .refused(why):
                credential = true
                message = why
            case let .server(why): message = why
            case let .unreachable(why): message = readable(why)
            }
            return Failure(stage: .incoming, message: message, credential: credential)
        } catch {
            return Failure(
                stage: .incoming, message: error.localizedDescription, credential: false)
        }
    }

    /// The outgoing half, shared by the kinds that have one.
    ///
    /// Greeted with the name real mail will be greeted with. Checking
    /// with `localhost` and sending with the address's domain is two
    /// different conversations, and a server that greylists on the
    /// greeting will pass one and hold the other.
    private static func verifySmtp(_ account: MailAccount, secret: String) async -> Failure? {
        let smtp = SMTPSession(host: account.smtpHost, port: account.smtpPort)
        do {
            try await smtp.connect(helo: AccountSender.helo(for: account))
            try await smtp.authenticate(
                user: account.loginName, secret: secret, oauth: account.auth == .oauth2)
            await smtp.close()
        } catch let e as SMTPSession.Failure {
            await smtp.close()
            return failure(.outgoing, e)
        } catch {
            await smtp.close()
            return Failure(
                stage: .outgoing, message: error.localizedDescription, credential: false)
        }
        return nil
    }

    private static func failure(_ stage: Failure.Stage, _ e: POP3Session.Failure) -> Failure {
        switch e {
        case let .refused(detail):
            return Failure(stage: stage, message: readable(detail), credential: true)
        case let .server(detail):
            return Failure(stage: stage, message: readable(detail), credential: false)
        case let .unreachable(detail):
            return Failure(stage: stage, message: readable(detail), credential: false)
        case .closed:
            return Failure(
                stage: stage, message: "the server closed the connection", credential: false)
        }
    }

    /// The server's own words where there are any, because a provider
    /// often says exactly what is wrong — and a message this app made
    /// up instead would be less useful.
    private static func failure(_ stage: Failure.Stage, _ e: IMAPSession.Failure) -> Failure {
        switch e {
        case let .refused(d):
            return Failure(stage: stage, message: readable(d), credential: true)
        case let .server(d):
            return Failure(stage: stage, message: readable(d), credential: false)
        case let .unreachable(d):
            return Failure(stage: stage, message: d, credential: false)
        case .closed:
            return Failure(
                stage: stage, message: "the server closed the connection", credential: false)
        case .timedOut:
            return Failure(stage: stage, message: "the server did not answer", credential: false)
        }
    }

    private static func failure(_ stage: Failure.Stage, _ e: SMTPSession.Failure) -> Failure {
        switch e {
        case let .refused(d):
            return Failure(stage: stage, message: readable(d), credential: true)
        case let .rejected(_, text, _):
            return Failure(stage: stage, message: readable(text), credential: false)
        case let .unreachable(d):
            return Failure(stage: stage, message: d, credential: false)
        case .closed:
            return Failure(
                stage: stage, message: "the server closed the connection", credential: false)
        }
    }

    /// Strip the response code a server puts in front of its reason.
    ///
    /// `[AUTHENTICATIONFAILED] Invalid credentials` reads better as
    /// `Invalid credentials`: the bracket is for programs.
    static func readable(_ detail: String) -> String {
        var s = detail.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("["), let close = s.firstIndex(of: "]") {
            s = String(s[s.index(after: close)...]).trimmingCharacters(in: .whitespaces)
        }
        // And the SMTP enhanced status code, which is the same idea.
        let parts = s.split(separator: " ", maxSplits: 1)
        if let first = parts.first, first.filter({ $0 == "." }).count == 2,
           first.allSatisfy({ $0.isNumber || $0 == "." }), parts.count == 2 {
            s = String(parts[1])
        }
        return s.isEmpty ? detail : s
    }
}
