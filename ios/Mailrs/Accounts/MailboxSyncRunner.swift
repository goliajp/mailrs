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

    /// Overridable so a test can pin the clock.
    nonisolated(unsafe) static var now: () -> Int64 = { Int64(Date().timeIntervalSince1970) }
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

            // A renumbering first: the folder's held rows are gone,
            // and applying the fetch on top of rows the server no
            // longer has any way to name would leave both.
            for folder in result.renumbered {
                AccountStore.dropFolder(account.id, folder)
            }
            // Then what became of the rest — read elsewhere, or gone.
            for (folder, answer) in result.refreshed {
                let decision = MailboxRefresh.decide(
                    asked: Set(held[folder] ?? []), answer: answer)
                AccountStore.deleteUids(account.id, folder, Array(decision.gone))
                AccountStore.setUidsSeen(account.id, folder, decision.flags)
            }
            AccountStore.upsertRows(result.rows)
            AccountStore.capAccount(account.id)
            AccountStore.saveMarks(result.marks, for: account.id)
            // **Only on the way out of a pass that worked.** A
            // timestamp written before the fetch, or after one that
            // failed, makes the screen say "just now" about mail it
            // never got — which is the one thing the line is there to
            // prevent.
            AccountStore.saveLastSync(account.id, now())
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

            AccountStore.upsertRows(fetched)
            AccountStore.capAccount(account.id)
            AccountStore.savePopSeen(account.id, seen)
            AccountStore.saveLastSync(account.id, now())
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
            AccountStore.upsertRows(rows)
            AccountStore.capAccount(account.id)
            AccountStore.saveLastSync(account.id, now())
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

    /// Fetch the mail **before** what is held, for one folder.
    ///
    /// Its own pass rather than part of the ordinary one: an ordinary
    /// pass answers "what is new", runs on a timer and on a pull, and
    /// must stay cheap. This one answers "what came before", runs only
    /// when somebody asks, and is allowed to reach.
    static func earlier(_ account: MailAccount, folder: String) async -> Outcome {
        guard account.incoming == .imap else {
            return Outcome(
                accountId: account.id, fetched: 0,
                failure: "Older mail is not available for this account yet")
        }
        guard let secret = AccountStore.secret(for: account.id) else {
            return Outcome(
                accountId: account.id, fetched: 0,
                failure: "Sign in again to read this account")
        }
        guard let mark = AccountStore.marks(for: account.id)[folder] else {
            return Outcome(
                accountId: account.id, fetched: 0, failure: "Fetch this mailbox first")
        }
        // Asked before the fetch, not after: at the ceiling the cap
        // would drop exactly what this pass went to get.
        guard !EarlierPlan.atCeiling(held: AccountStore.count(account.id)) else {
            return Outcome(
                accountId: account.id, fetched: 0,
                failure: "This device is holding as much of this account as it can")
        }
        // A mark from before this was recorded: anchor from what is
        // actually held rather than refusing, so an account that has
        // been syncing for weeks does not have to start over.
        var anchor = mark.lowestUid
        if anchor == 0 {
            let held = AccountStore.rows()
                .filter { $0.accountId == account.id && $0.folder == folder }
                .map(\.uid)
            guard let lowest = held.min() else {
                return Outcome(
                    accountId: account.id, fetched: 0, failure: "Fetch this mailbox first")
            }
            anchor = lowest
        }
        let ask = EarlierPlan.decide(lowestHeldUid: anchor, span: mark.earlierSpan)
        guard let range = ask.range else {
            return Outcome(accountId: account.id, fetched: 0, failure: nil)
        }

        let session = openImap(account.imapHost, account.imapPort)
        do {
            try await session.connect()
            if account.auth == .oauth2 {
                try await session.authenticateXOAuth2(user: account.loginName, token: secret)
            } else {
                try await session.login(user: account.loginName, password: secret)
            }
            let (validity, _) = try await session.select(folder)
            guard validity == mark.uidValidity else {
                // The folder was renumbered while this was being asked.
                // Every uid held means something else now, so reaching
                // below one of them would fetch whatever happens to be
                // there — the ordinary pass re-anchors, and this one
                // steps aside for it.
                await session.close()
                return Outcome(
                    accountId: account.id, fetched: 0,
                    failure: "This mailbox was rebuilt — fetch it again")
            }
            let fetched = try await session.fetchHeaders(range: range)
            await session.close()

            let rows = fetched.map { message in
                MailboxRow(
                    accountId: account.id, uid: message.uid, folder: folder,
                    seen: message.seen,
                    sender: MessageHeaders.senderName(message.headers.from),
                    subject: message.headers.subject, date: message.date,
                    messageId: message.headers.messageId, size: message.size)
            }
            AccountStore.upsertRows(rows)
            AccountStore.capAccount(account.id)

            // The **range** that was asked about, not the lowest that
            // came back: a range that is all gaps returns nothing, and
            // anchoring on what returned would ask the same empty
            // question forever.
            let reached = UInt32(range.split(separator: ":").first.map(String.init) ?? "") ?? anchor
            var marks = AccountStore.marks(for: account.id)
            marks[folder] = FolderMark(
                uidValidity: mark.uidValidity, highestUid: mark.highestUid,
                lowestUid: reached,
                earlierSpan: EarlierPlan.nextSpan(mark.earlierSpan, returned: rows.count))
            AccountStore.saveMarks(marks, for: account.id)
            return Outcome(accountId: account.id, fetched: rows.count, failure: nil)
        } catch {
            await session.close()
            return Outcome(
                accountId: account.id, fetched: 0, failure: "Could not reach the server")
        }
    }
}
