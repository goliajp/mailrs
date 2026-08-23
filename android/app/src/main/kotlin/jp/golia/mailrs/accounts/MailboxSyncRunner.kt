package jp.golia.mailrs.accounts

/**
 * Runs one sync pass over a connected account.
 *
 * The pass itself — which folders, from which uid, what to do about a
 * renumbering — is [MailboxSync], which needs no network. This is the
 * part that opens sockets, and it holds no decisions of its own.
 */
object MailboxSyncRunner {
    /** What one account's pass came to, for the screen to report. */
    data class Outcome(
        val accountId: String,
        val fetched: Int,
        /**
         * Why nothing came back, in a person's words. `null` when the
         * pass ran, **including when it fetched nothing** — no new mail
         * is not a failure and must not be reported as one.
         */
        val failure: String? = null,
    )

    /**
     * Sync one account, folding what comes back into [store].
     *
     * Each account is on its own: one unreachable server must not stop
     * the others, in the same way one broken folder does not stop an
     * account's other folders.
     */
    suspend fun run(account: MailAccount, store: AccountStore): Outcome {
        val secret = store.secret(account.id)
            ?: return Outcome(account.id, 0, "Sign in again to read this account")
        val session = ImapSession(account.imapHost, account.imapPort)
        return try {
            session.connect()
            if (account.auth == MailProvider.AuthKind.OAUTH2) {
                session.authenticateXOAuth2(account.loginName, secret)
            } else {
                session.login(account.loginName, secret)
            }
            val result = MailboxSync.pass(account, session, store.marksFor(account.id))
            session.close()

            var held = store.rows()
            // A renumbering first: the folder's held rows are gone, and
            // applying the fetch on top of rows the server no longer has
            // any way to name would leave both.
            for (folder in result.renumbered) {
                held = MailboxApply.replacingFolder(held, account.id, folder, emptyList())
            }
            held = MailboxApply.apply(held, result.rows)
            store.saveRows(held)
            store.saveMarksFor(account.id, result.marks)
            Outcome(account.id, result.rows.size)
        } catch (e: ImapSession.Failure) {
            session.close()
            Outcome(account.id, 0, AccountConnection.readable(e.message ?: "unknown"))
        } catch (e: Exception) {
            session.close()
            Outcome(account.id, 0, "Could not reach the server")
        }
    }
}
