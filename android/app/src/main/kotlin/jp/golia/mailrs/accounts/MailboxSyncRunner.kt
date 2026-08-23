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
        if (account.incoming == Incoming.POP3) return runPop3(account, secret, store)
        if (account.incoming == Incoming.JMAP) return runJmap(account, secret, store)
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

    /**
     * A POP3 pass.
     *
     * One folder, called INBOX because that is what it is and a list
     * needs a name for it; no server-side flags, so every row arrives
     * unread and stays that way until this device says otherwise; and
     * only the headers are fetched, because downloading a mailbox to
     * show a list is somebody's data allowance.
     */
    private suspend fun runPop3(
        account: MailAccount,
        secret: String,
        store: AccountStore,
    ): Outcome {
        val session = Pop3Session(account.imapHost, account.imapPort)
        return try {
            session.connect()
            session.login(account.loginName, secret)
            // **Listed once.** Asking twice is two round trips and two
            // answers: a message deleted between them renumbers the
            // rest, and the plan would then name numbers that mean
            // something else.
            val listing = session.uidls()
            val plan = Pop3Plan.decide(listing, store.popSeen(account.id))
            val byNumber = listing.associateBy { it.number }
            val fetched = mutableListOf<MailboxRow>()
            val seen = plan.keep.toMutableSet()
            for (number in plan.fetch) {
                val id = byNumber[number]?.id ?: continue
                val headers = MessageHeaders.parse(Wire.utf8(session.headers(number)))
                fetched.add(
                    MailboxRow(
                        accountId = account.id,
                        // The uidl is the identity, so it is what the row
                        // is keyed on — folded to a number because a row
                        // id is one, and because the same folding is what
                        // orders threads elsewhere.
                        uid = foldedUid(id),
                        folder = "INBOX",
                        seen = false,
                        sender = headers.from,
                        subject = headers.subject,
                        date = MailDate.epochSeconds(headers.date),
                        messageId = headers.messageId,
                    ),
                )
                seen.add(id)
            }
            // Ended properly: a POP3 server holds an exclusive lock on
            // the mailbox for the length of a session, and one dropped
            // without QUIT keeps it until the timeout — during which the
            // person's other device cannot read their mail either.
            session.quit()
            session.close()

            store.saveRows(MailboxApply.apply(store.rows(), fetched))
            store.savePopSeen(account.id, seen)
            Outcome(account.id, fetched.size)
        } catch (e: Pop3Session.Failure) {
            session.close()
            Outcome(account.id, 0, AccountConnection.readable(e.message ?: "unknown"))
        } catch (e: Exception) {
            session.close()
            Outcome(account.id, 0, "Could not reach the server")
        }
    }

    /** A uidl is text; a row id is a number. FNV-1a, as elsewhere. */
    internal fun foldedUid(uidl: String): Long {
        var h = -0x340d631b7bdddcdbL
        for (b in uidl.toByteArray()) {
            h = h xor (b.toLong() and 0xff)
            h *= 0x100000001b3L
        }
        return h ushr 1
    }

    /**
     * A JMAP pass.
     *
     * One request for the session object and one for the mail, and the
     * second is one round trip because the query feeds the get through
     * a back-reference. There is no folder here either: JMAP has
     * mailboxes, but a list of the newest mail across all of them is
     * what a merged view wants, so the rows say `INBOX` and mean "this
     * account".
     */
    private suspend fun runJmap(
        account: MailAccount,
        secret: String,
        store: AccountStore,
    ): Outcome {
        val session = JmapSession(account.imapHost)
        return try {
            // A token goes as a Bearer, so the login name is left empty
            // for OAuth accounts; a password goes as Basic with it.
            val user = when (account.auth) {
                MailProvider.AuthKind.OAUTH2 -> ""
                else -> account.loginName
            }
            val found = session.session(user, secret)
            val rows = session.newest(found, user, secret).map { email ->
                MailboxRow(
                    accountId = account.id,
                    uid = foldedUid(email.id),
                    folder = "INBOX",
                    seen = email.seen,
                    sender = email.sender,
                    subject = email.subject,
                    date = email.receivedAt,
                    messageId = email.messageId,
                )
            }
            store.saveRows(MailboxApply.apply(store.rows(), rows))
            Outcome(account.id, rows.size)
        } catch (e: JmapSession.Failure) {
            Outcome(account.id, 0, e.message ?: "Could not reach the server")
        } catch (e: Exception) {
            Outcome(account.id, 0, "Could not reach the server")
        }
    }
}
