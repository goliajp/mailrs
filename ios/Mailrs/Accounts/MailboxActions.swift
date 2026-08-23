import Foundation

/// Doing something to one message on the server.
///
/// Separate from `MailboxSyncRunner` because these run on a tap rather
/// than on a pass, and because each one has to leave the device's copy
/// and the server agreeing — an action that changes only the screen is
/// an action that undoes itself on the next fetch.
enum MailboxActions {
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
            AccountStore.saveRows(AccountStore.rows().filter { $0.id != row.id })
            return .done
        }
    }

    /// Put a message back to unread, on the server and here.
    static func markUnread(_ row: MailboxRow, from account: MailAccount) async -> Outcome {
        guard account.incoming == .imap else {
            // POP3 has no server-side flags at all, so unread is purely
            // local — and saying so is better than a button that looks
            // like it did something remote.
            AccountStore.saveRows(setSeen(AccountStore.rows(), id: row.id, seen: false))
            return .done
        }
        return await withImap(account) { session in
            _ = try await session.select(row.folder)
            try await session.markUnseen(uid: row.uid)
            AccountStore.saveRows(setSeen(AccountStore.rows(), id: row.id, seen: false))
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
        _ account: MailAccount, _ body: (IMAPSession) async throws -> Outcome
    ) async -> Outcome {
        guard let secret = AccountStore.secret(for: account.id) else {
            return .failed("Sign in again to change this account")
        }
        let session = IMAPSession(host: account.imapHost, port: account.imapPort)
        do {
            try await session.connect()
            if account.auth == .oauth2 {
                try await session.authenticateXOAuth2(user: account.loginName, token: secret)
            } else {
                try await session.login(user: account.loginName, password: secret)
            }
            let outcome = try await body(session)
            await session.close()
            return outcome
        } catch {
            await session.close()
            return .failed("Could not reach the server")
        }
    }
}
