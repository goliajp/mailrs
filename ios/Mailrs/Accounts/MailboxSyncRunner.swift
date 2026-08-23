import Foundation

/// Runs one sync pass over every connected account.
///
/// The pass itself — which folders, from which uid, what to do about a
/// renumbering — is `MailboxSync`, which needs no network. This is the
/// part that opens sockets, and it holds no decisions of its own.
enum MailboxSyncRunner {
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
        let session = IMAPSession(host: account.imapHost, port: account.imapPort)
        do {
            try await session.connect()
            if account.auth == .oauth2 {
                try await session.authenticateXOAuth2(user: account.loginName, token: secret)
            } else {
                try await session.login(user: account.loginName, password: secret)
            }
            let result = try await MailboxSync.pass(
                account: account, session: session,
                marks: AccountStore.marks(for: account.id))
            await session.close()

            var held = AccountStore.rows()
            // A renumbering first: the folder's held rows are gone,
            // and applying the fetch on top of rows the server no
            // longer has any way to name would leave both.
            for folder in result.renumbered {
                held = MailboxApply.replacingFolder(
                    held: held, accountId: account.id, folder: folder, with: [])
            }
            held = MailboxApply.apply(held: held, fetched: result.rows)
            AccountStore.saveRows(held)
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
}
