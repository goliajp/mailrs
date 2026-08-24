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
    /** Where IMAP connections are kept between taps. */
    internal var pool: ImapPool = ImapPool.shared
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
        if (account.incoming == Incoming.POP3) return deletePop3(account, row, store)
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
            store.deleteRow(row)
            Outcome.Done
        }
    }

    /** Put a message back to unread, on the server and here. */
    suspend fun markUnread(account: MailAccount, row: MailboxRow, store: AccountStore): Outcome {
        if (account.incoming != Incoming.IMAP) {
            // POP3 has no server-side flags at all, so unread is purely
            // local — and saying so is better than a button that looks
            // like it did something remote.
            store.setRowSeen(row, false)
            return Outcome.Done
        }
        return withImap(account, store) { session ->
            session.select(row.folder)
            session.markUnseen(row.uid)
            store.setRowSeen(row, false)
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
        // Through the pool: one tap is one command, and paying for a
        // TLS handshake and a LOGIN each time is most of the wait.
        // The session is **not** closed here — that is the point of it.
        return try {
            pool.use(account, secret) { session -> body(session) }
        } catch (e: ImapSession.Failure) {
            Outcome.Failed(AccountConnection.readable(e.message ?: "unknown"))
        } catch (e: Exception) {
            Outcome.Failed("Could not reach the server")
        }
    }

    /**
     * Delete from a POP3 mailbox.
     *
     * Three things that are not true of IMAP:
     *
     * - **The number is only valid in this session.** POP3 renumbers
     *   every time, so the uidl has to be looked up now; a stored
     *   number would delete whatever happens to be in that position.
     * - **`DELE` does not delete.** The server acts at `QUIT`, so a
     *   session dropped after `DELE` leaves the mailbox untouched.
     * - **A message already gone is a success, not an error.** It was
     *   deleted from another device, and telling somebody their delete
     *   failed when the thing is gone is a lie that makes them try
     *   again.
     */
    private suspend fun deletePop3(
        account: MailAccount,
        row: MailboxRow,
        store: AccountStore,
    ): Outcome {
        val secret = store.secret(account.id)
            ?: return Outcome.Failed("Sign in again to change this account")
        val session = openPop3(account.imapHost, account.imapPort)
        return try {
            session.connect()
            session.login(account.loginName, secret)
            val listing = session.uidls()
            // The row's uid is the folded uidl, which is how the two
            // are matched — the uidl itself is text and a row id is a
            // number.
            val target = listing.firstOrNull {
                MailboxSyncRunner.foldedUid(it.id) == row.uid
            }
            if (target != null) session.delete(target.number)
            session.quit()
            session.close()
            store.deleteRow(row)
            // And forgotten, so a mailbox that still lists it after a
            // failed QUIT is fetched again rather than skipped forever.
            target?.let { store.savePopSeen(account.id, store.popSeen(account.id) - it.id) }
            Outcome.Done
        } catch (e: Pop3Session.Failure) {
            session.close()
            Outcome.Failed(AccountConnection.readable(e.message ?: "unknown"))
        } catch (e: Exception) {
            session.close()
            Outcome.Failed("Could not reach the server")
        }
    }

    /** How a POP3 session is made; injectable like the others. */
    internal var openPop3: (String, Int) -> Pop3Session = { host, port -> Pop3Session(host, port) }
}
