import Foundation

/// Doing something to one message on the server.
///
/// Separate from `MailboxSyncRunner` because these run on a tap rather
/// than on a pass, and because each one has to leave the device's copy
/// and the server agreeing — an action that changes only the screen is
/// an action that undoes itself on the next fetch.
enum MailboxActions {
    /// Where IMAP connections are kept between taps.
    ///
    /// Injectable for the same reason `MailboxSyncRunner`'s session is:
    /// the rule that matters here — **the row goes only after the
    /// server says it has** — is about the order of a network call and
    /// a write, and neither half alone can show it.
    nonisolated(unsafe) static var pool: ImapPool = .shared

    /// How a POP3 session is made; injectable like the other.
    nonisolated(unsafe) static var openPop3: (String, UInt16) -> POP3Session = {
        POP3Session(host: $0, port: $1)
    }
    enum Outcome: Equatable {
        case done
        case failed(String)
    }

    /// Move a message to the account's trash.
    ///
    /// POP3 and JMAP are not offered this: POP3's delete is a
    /// different command with different semantics, and JMAP's is an
    /// `Email/set` call. Refusing plainly beats doing nothing quietly.
    static func delete(_ row: MailboxRow, from account: MailAccount) async -> Outcome {
        if account.incoming == .pop3 { return await deletePop3(row, from: account) }
        guard account.incoming == .imap else {
            return .failed("Deleting is not supported for this account yet")
        }
        return await withImap(account) { session in
            guard let trash = TrashFolder.pick(try await session.list()) else {
                return .failed("This account has no trash folder")
            }
            _ = try await session.select(row.folder)
            try await session.moveTo(
                uid: row.uid, folder: trash, capabilities: try await session.capabilities())
            // Gone from the device too, and only after the server said
            // so: a row removed first and a move that then failed is a
            // message the person cannot see and has not lost.
            AccountStore.deleteRow(row)
            return .done
        }
    }

    /// Put a message back to unread, on the server and here.
    static func markUnread(_ row: MailboxRow, from account: MailAccount) async -> Outcome {
        guard account.incoming == .imap else {
            // POP3 has no server-side flags at all, so unread is purely
            // local — and saying so is better than a button that looks
            // like it did something remote.
            AccountStore.setRowSeen(row, false)
            return .done
        }
        return await withImap(account) { session in
            _ = try await session.select(row.folder)
            try await session.markUnseen(uid: row.uid)
            AccountStore.setRowSeen(row, false)
            return .done
        }
    }

    private static func setSeen(_ rows: [MailboxRow], id: String, seen: Bool) -> [MailboxRow] {
        rows.map { row in
            guard row.id == id else { return row }
            var changed = row
            changed.seen = seen
            return changed
        }
    }

    private static func withImap(
        _ account: MailAccount, _ body: sending @escaping (IMAPSession) async throws -> Outcome
    ) async -> Outcome {
        guard let secret = AccountStore.secret(for: account.id) else {
            return .failed("Sign in again to change this account")
        }
        // Through the pool: one tap is one command, and paying for a
        // TLS handshake and a LOGIN each time is most of the wait. The
        // session is **not** closed here — that is the point of it.
        do {
            return try await pool.use(account, secret: secret) { session in
                try await body(session)
            }
        } catch {
            return .failed("Could not reach the server")
        }
    }

    /// Delete from a POP3 mailbox.
    ///
    /// Three things that are not true of IMAP:
    ///
    /// - **The number is only valid in this session.** POP3 renumbers
    ///   every time, so the uidl has to be looked up now; a stored
    ///   number would delete whatever happens to be in that position.
    /// - **`DELE` does not delete.** The server acts at `QUIT`, so a
    ///   session dropped after `DELE` leaves the mailbox untouched.
    /// - **A message already gone is a success, not an error.** It was
    ///   deleted from another device, and telling somebody their
    ///   delete failed when the thing is gone is a lie that makes them
    ///   try again.
    private static func deletePop3(_ row: MailboxRow, from account: MailAccount) async -> Outcome {
        guard let secret = AccountStore.secret(for: account.id) else {
            return .failed("Sign in again to change this account")
        }
        let session = openPop3(account.imapHost, account.imapPort)
        do {
            try await session.connect()
            try await session.login(user: account.loginName, password: secret)
            let listing = try await session.uidls()
            // The row's uid is the folded uidl, which is how the two
            // are matched — the uidl itself is text and a row id is a
            // number.
            let target = listing.first { MailboxSyncRunner.foldedUid($0.id) == row.uid }
            if let target { try await session.delete(number: target.number) }
            await session.quit()
            await session.close()
            AccountStore.deleteRow(row)
            // And forgotten, so a mailbox that still lists it after a
            // failed QUIT is fetched again rather than skipped forever.
            if let target {
                AccountStore.savePopSeen(
                    account.id, AccountStore.popSeen(account.id).subtracting([target.id]))
            }
            return .done
        } catch {
            await session.close()
            return .failed("Could not reach the server")
        }
    }
}
