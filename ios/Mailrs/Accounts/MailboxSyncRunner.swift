import Foundation

/// Runs one sync pass over every connected account.
///
/// The pass itself — which folders, from which uid, what to do about a
/// renumbering — is `MailboxSync`, which needs no network. This is the
/// part that opens sockets, and it holds no decisions of its own.
enum MailboxSyncRunner {
    /// How a session is made.
    ///
    /// Injectable so a test can join the wire to the store: every rule
    /// above the socket is asserted and every socket conversation is
    /// asserted, and until this existed the two had never been checked
    /// together. A pass that talks correctly and files the answer in
    /// the wrong place passes both halves and shows nobody their mail.
    nonisolated(unsafe) static var openImap: (String, UInt16) -> IMAPSession = {
        IMAPSession(host: $0, port: $1)
    }

    /// The same, for the other two protocols.
    nonisolated(unsafe) static var openPop3: (String, UInt16) -> POP3Session = {
        POP3Session(host: $0, port: $1)
    }
    nonisolated(unsafe) static var openJmap: (String) -> JMAPClient = { JMAPClient(host: $0) }
    /// What one account's pass came to, for the screen to report.
    struct Outcome: Equatable {
        let accountId: String
        let fetched: Int
        /// Why nothing came back, in a person's words. `nil` when the
        /// pass ran, **including when it fetched nothing** — no new
        /// mail is not a failure and must not be reported as one.
        let failure: String?
    }

    /// Sync one account, folding what comes back into the store.
    ///
    /// Each account is on its own: one unreachable server must not
    /// stop the others, in the same way one broken folder does not
    /// stop an account's other folders.
    @discardableResult
    static func run(_ account: MailAccount) async -> Outcome {
        guard let secret = AccountStore.secret(for: account.id) else {
            return Outcome(
                accountId: account.id, fetched: 0,
                failure: "Sign in again to read this account")
        }
        if account.incoming == .pop3 { return await runPop3(account, secret: secret) }
        if account.incoming == .jmap { return await runJmap(account, secret: secret) }
        let session = openImap(account.imapHost, account.imapPort)
        do {
            try await session.connect()
            if account.auth == .oauth2 {
                try await session.authenticateXOAuth2(user: account.loginName, token: secret)
            } else {
                try await session.login(user: account.loginName, password: secret)
            }
            // What this device already holds, per folder, so the pass
            // can ask what became of it.
            var held: [String: [UInt32]] = [:]
            for row in AccountStore.rows() where row.accountId == account.id {
                held[row.folder, default: []].append(row.uid)
            }
            let result = try await MailboxSync.pass(
                account: account, session: session,
                marks: AccountStore.marks(for: account.id), held: held)
            await session.close()

            var kept = AccountStore.rows()
            // A renumbering first: the folder's held rows are gone,
            // and applying the fetch on top of rows the server no
            // longer has any way to name would leave both.
            for folder in result.renumbered {
                kept = MailboxApply.replacingFolder(
                    held: kept, accountId: account.id, folder: folder, with: [])
            }
            // Then what became of the rest — read elsewhere, or gone.
            for (folder, answer) in result.refreshed {
                kept = MailboxRefresh.apply(
                    held: kept, accountId: account.id, folder: folder,
                    asked: Set(held[folder] ?? []), answer: answer)
            }
            kept = MailboxApply.apply(held: kept, fetched: result.rows)
            AccountStore.saveRows(kept)
            AccountStore.saveMarks(result.marks, for: account.id)
            return Outcome(accountId: account.id, fetched: result.rows.count, failure: nil)
        } catch let e as IMAPSession.Failure {
            await session.close()
            return Outcome(
                accountId: account.id, fetched: 0,
                failure: AccountConnection.readable(detail(e)))
        } catch {
            await session.close()
            return Outcome(accountId: account.id, fetched: 0, failure: "Could not reach the server")
        }
    }

    private static func detail(_ e: IMAPSession.Failure) -> String {
        switch e {
        case let .refused(d): return d
        case let .server(d): return d
        case let .unreachable(d): return d
        case .closed: return "the server closed the connection"
        case .timedOut: return "the server did not answer"
        }
    }

    /// A POP3 pass.
    ///
    /// One folder, called INBOX because that is what it is and a list
    /// needs a name for it; no server-side flags, so every row arrives
    /// unread and stays that way until this device says otherwise; and
    /// only the headers are fetched, because downloading a mailbox to
    /// show a list is somebody's data allowance.
    private static func runPop3(_ account: MailAccount, secret: String) async -> Outcome {
        let session = openPop3(account.imapHost, account.imapPort)
        do {
            try await session.connect()
            try await session.login(user: account.loginName, password: secret)
            // **Listed once.** Asking twice is two round trips and two
            // answers: a message deleted between them renumbers the
            // rest, and the plan would then name numbers that mean
            // something else.
            let listing = try await session.uidls()
            let plan = POP3Plan.decide(server: listing, seen: AccountStore.popSeen(account.id))
            var byNumber: [Int: String] = [:]
            for one in listing { byNumber[one.number] = one.id }

            var fetched: [MailboxRow] = []
            var seen = plan.keep
            for number in plan.fetch {
                guard let id = byNumber[number] else { continue }
                let headers = MessageHeaders.parse(try await session.headers(number: number))
                fetched.append(
                    MailboxRow(
                        accountId: account.id,
                        // The uidl is the identity, so it is what the
                        // row is keyed on — folded to a number because
                        // a row id is one.
                        uid: foldedUid(id),
                        folder: "INBOX",
                        seen: false,
                        sender: headers.from,
                        subject: headers.subject,
                        date: MailDate.epochSeconds(headers.date),
                        messageId: headers.messageId))
                seen.insert(id)
            }
            // Ended properly: a POP3 server holds an exclusive lock on
            // the mailbox for the length of a session, and one dropped
            // without QUIT keeps it until the timeout — during which
            // the person's other device cannot read their mail either.
            await session.quit()
            await session.close()

            AccountStore.saveRows(MailboxApply.apply(held: AccountStore.rows(), fetched: fetched))
            AccountStore.savePopSeen(account.id, seen)
            return Outcome(accountId: account.id, fetched: fetched.count, failure: nil)
        } catch {
            await session.close()
            return Outcome(
                accountId: account.id, fetched: 0, failure: "Could not read this mailbox")
        }
    }

    /// A JMAP pass: the session object, then the mail in one round trip.
    private static func runJmap(_ account: MailAccount, secret: String) async -> Outcome {
        // A token goes as a Bearer, so the login name is left empty for
        // OAuth accounts; a password goes as Basic with it.
        var user = account.loginName
        if account.auth == .oauth2 { user = "" }
        let client = openJmap(account.imapHost)
        do {
            let found = try await client.session(user: user, secret: secret)
            let rows = try await client.newest(session: found, user: user, secret: secret)
                .map { email in
                    MailboxRow(
                        accountId: account.id,
                        uid: foldedUid(email.id),
                        folder: "INBOX",
                        seen: email.seen,
                        sender: email.sender,
                        subject: email.subject,
                        date: email.receivedAt,
                        messageId: email.messageId)
                }
            AccountStore.saveRows(MailboxApply.apply(held: AccountStore.rows(), fetched: rows))
            return Outcome(accountId: account.id, fetched: rows.count, failure: nil)
        } catch let e as JMAPClient.Failure {
            return Outcome(accountId: account.id, fetched: 0, failure: explain(e))
        } catch {
            return Outcome(
                accountId: account.id, fetched: 0, failure: "Could not reach the server")
        }
    }

    private static func explain(_ e: JMAPClient.Failure) -> String {
        switch e {
        case let .unreachable(why): return AccountConnection.readable(why)
        case let .refused(why): return why
        case let .server(why): return why
        }
    }

    /// A uidl is text; a row id is a number. FNV-1a, as elsewhere.
    static func foldedUid(_ id: String) -> UInt32 {
        var h: UInt64 = 0xcbf2_9ce4_8422_2325
        for b in id.utf8 {
            h ^= UInt64(b)
            h = h &* 0x100_0000_01b3
        }
        return UInt32(truncatingIfNeeded: h >> 1)
    }
}
