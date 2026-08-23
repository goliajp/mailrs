package jp.golia.mailrs.accounts

/**
 * Doing something to one message on the server.
 *
 * Separate from [MailboxSyncRunner] because these run on a tap rather
 * than on a pass, and because each one has to leave the device's copy
 * and the server agreeing — an action that changes only the screen is
 * an action that undoes itself on the next fetch.
 */
object MailboxActions {
    /**
     * How a session is made.
     *
     * Injectable for the same reason [MailboxSyncRunner]'s is: the
     * rule that matters here — **the row goes only after the server
     * says it has** — is about the order of a network call and a
     * write, and neither half alone can show it.
     */
    internal var openImap: (String, Int) -> ImapSession = { host, port -> ImapSession(host, port) }
    sealed class Outcome {
        object Done : Outcome()
        data class Failed(val why: String) : Outcome()
    }

    /**
     * Move a message to the account's trash.
     *
     * POP3 and JMAP are not offered this: POP3's delete is a different
     * command with different semantics, and JMAP's is a `Email/set`
     * call. Refusing plainly beats doing nothing quietly.
     */
    suspend fun delete(account: MailAccount, row: MailboxRow, store: AccountStore): Outcome {
        if (account.incoming != Incoming.IMAP) {
            return Outcome.Failed("Deleting is not supported for this account yet")
        }
        return withImap(account, store) { session ->
            val trash = TrashFolder.pick(session.list())
                ?: return@withImap Outcome.Failed("This account has no trash folder")
            session.select(row.folder)
            session.moveTo(row.uid, trash, session.capabilities())
            // Gone from the device too, and only after the server said
            // so: a row removed first and a move that then failed is a
            // message the person cannot see and has not lost.
            store.saveRows(store.rows().filterNot { it.id == row.id })
            Outcome.Done
        }
    }

    /** Put a message back to unread, on the server and here. */
    suspend fun markUnread(account: MailAccount, row: MailboxRow, store: AccountStore): Outcome {
        if (account.incoming != Incoming.IMAP) {
            // POP3 has no server-side flags at all, so unread is purely
            // local — and saying so is better than a button that looks
            // like it did something remote.
            store.saveRows(setSeen(store.rows(), row.id, false))
            return Outcome.Done
        }
        return withImap(account, store) { session ->
            session.select(row.folder)
            session.markUnseen(row.uid)
            store.saveRows(setSeen(store.rows(), row.id, false))
            Outcome.Done
        }
    }

    private fun setSeen(rows: List<MailboxRow>, id: String, seen: Boolean) =
        rows.map { row ->
            when (row.id) {
                id -> row.copy(seen = seen)
                else -> row
            }
        }

    private suspend fun withImap(
        account: MailAccount,
        store: AccountStore,
        body: suspend (ImapSession) -> Outcome,
    ): Outcome {
        val secret = store.secret(account.id)
            ?: return Outcome.Failed("Sign in again to change this account")
        val session = openImap(account.imapHost, account.imapPort)
        return try {
            session.connect()
            if (account.auth == MailProvider.AuthKind.OAUTH2) {
                session.authenticateXOAuth2(account.loginName, secret)
            } else {
                session.login(account.loginName, secret)
            }
            val outcome = body(session)
            session.close()
            outcome
        } catch (e: ImapSession.Failure) {
            session.close()
            Outcome.Failed(AccountConnection.readable(e.message ?: "unknown"))
        } catch (e: Exception) {
            session.close()
            Outcome.Failed("Could not reach the server")
        }
    }
}
