package jp.golia.mailrs.accounts

/**
 * Runs one sync pass over a connected account.
 *
 * The pass itself — which folders, from which uid, what to do about a
 * renumbering — is [MailboxSync], which needs no network. This is the
 * part that opens sockets, and it holds no decisions of its own.
 */
object MailboxSyncRunner {
    /**
     * How a session is made.
     *
     * Injectable so a test can join the wire to the store: every rule
     * above the socket is asserted and every socket conversation is
     * asserted, and until this existed the two had never been checked
     * together. A pass that talks correctly and files the answer in the
     * wrong place passes both halves.
     */
    internal var openImap: (String, Int) -> ImapSession = { host, port -> ImapSession(host, port) }

    /** The same, for the other two protocols. */
    internal var openPop3: (String, Int) -> Pop3Session = { host, port -> Pop3Session(host, port) }
    internal var openJmap: (String) -> JmapSession = { host -> JmapSession(host) }
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
        val session = openImap(account.imapHost, account.imapPort)
        return try {
            session.connect()
            if (account.auth == MailProvider.AuthKind.OAUTH2) {
                session.authenticateXOAuth2(account.loginName, secret)
            } else {
                session.login(account.loginName, secret)
            }
            // What this device already holds, per folder, so the pass
            // can ask what became of it.
            val mine = store.rows().filter { it.accountId == account.id }
            val held = mine.groupBy { it.folder }.mapValues { (_, rows) -> rows.map { it.uid } }
            val result = MailboxSync.pass(account, session, store.marksFor(account.id), held)
            session.close()

            // A renumbering first: the folder's held rows are gone, and
            // applying the fetch on top of rows the server no longer has
            // any way to name would leave both.
            for (folder in result.renumbered) {
                store.dropFolder(account.id, folder)
            }
            // Then what became of the rest — read elsewhere, or gone.
            for ((folder, answer) in result.refreshed) {
                val decision = MailboxRefresh.decide(held[folder].orEmpty().toSet(), answer)
                store.deleteUids(account.id, folder, decision.gone)
                store.setUidsSeen(account.id, folder, decision.flags)
            }
            store.upsertRows(result.rows)
            store.capAccount(account.id)
            store.saveMarksFor(account.id, result.marks)
            // **Only on the way out of a pass that worked.** A
            // timestamp written before the fetch, or after one that
            // failed, makes the screen say "just now" about mail it
            // never got — which is the one thing the line is there to
            // prevent.
            store.saveLastSync(account.id, now())
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
        val session = openPop3(account.imapHost, account.imapPort)
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

            store.upsertRows(fetched)
            store.capAccount(account.id)
            store.savePopSeen(account.id, seen)
            store.saveLastSync(account.id, now())
            Outcome(account.id, fetched.size)
        } catch (e: Pop3Session.Failure) {
            session.close()
            Outcome(account.id, 0, AccountConnection.readable(e.message ?: "unknown"))
        } catch (e: Exception) {
            session.close()
            Outcome(account.id, 0, "Could not reach the server")
        }
    }

    /** Overridable so a test can pin the clock. */
    internal var now: () -> Long = { System.currentTimeMillis() / 1000 }

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
        val session = openJmap(account.imapHost)
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
            store.upsertRows(rows)
            store.capAccount(account.id)
            store.saveLastSync(account.id, now())
            Outcome(account.id, rows.size)
        } catch (e: JmapSession.Failure) {
            Outcome(account.id, 0, e.message ?: "Could not reach the server")
        } catch (e: Exception) {
            Outcome(account.id, 0, "Could not reach the server")
        }
    }

    /**
     * Fetch the mail **before** what is held, for one folder.
     *
     * Its own pass rather than part of the ordinary one: an ordinary
     * pass answers "what is new", runs on a timer and on a pull, and
     * must stay cheap. This one answers "what came before", runs only
     * when somebody asks, and is allowed to reach.
     */
    suspend fun earlier(
        account: MailAccount,
        folder: String,
        store: AccountStore,
    ): Outcome {
        if (account.incoming != Incoming.IMAP) {
            return Outcome(account.id, 0, "Older mail is not available for this account yet")
        }
        val secret = store.secret(account.id)
            ?: return Outcome(account.id, 0, "Sign in again to read this account")
        val mark = store.marksFor(account.id)[folder]
            ?: return Outcome(account.id, 0, "Fetch this mailbox first")
        // Asked before the fetch, not after: at the ceiling the cap
        // would drop exactly what this pass went to get.
        if (EarlierPlan.atCeiling(store.count(account.id))) {
            return Outcome(
                account.id, 0,
                "This device is holding as much of this account as it can",
            )
        }
        val anchor = when (mark.lowestUid) {
            // A mark from before this was recorded: anchor from what is
            // actually held rather than refusing, so an account that
            // has been syncing for weeks does not have to start over.
            0L -> store.rows()
                .filter { it.accountId == account.id && it.folder == folder }
                .minOfOrNull { it.uid } ?: return Outcome(account.id, 0, "Fetch this mailbox first")
            else -> mark.lowestUid
        }
        val ask = EarlierPlan.decide(anchor, mark.earlierSpan)
        val range = ask.range
            ?: return Outcome(account.id, 0, null)

        val session = openImap(account.imapHost, account.imapPort)
        return try {
            session.connect()
            if (account.auth == MailProvider.AuthKind.OAUTH2) {
                session.authenticateXOAuth2(account.loginName, secret)
            } else {
                session.login(account.loginName, secret)
            }
            val (validity, _) = session.select(folder)
            if (validity != mark.uidValidity) {
                // The folder was renumbered while this was being asked.
                // Every uid held means something else now, so reaching
                // below one of them would fetch whatever happens to be
                // there — the ordinary pass re-anchors, and this one
                // steps aside for it.
                session.close()
                return Outcome(account.id, 0, "This mailbox was rebuilt — fetch it again")
            }
            val fetched = session.fetchHeaders(range)
            session.close()

            val rows = fetched.map { message ->
                MailboxRow(
                    accountId = account.id,
                    uid = message.uid,
                    folder = folder,
                    seen = message.seen,
                    sender = MessageHeaders.senderName(message.headers.from),
                    subject = message.headers.subject,
                    date = message.date,
                    messageId = message.headers.messageId,
                    size = message.size,
                )
            }
            store.upsertRows(rows)
            store.capAccount(account.id)
            val reached = range.substringBefore(':').toLongOrNull() ?: anchor
            store.saveMarksFor(
                account.id,
                store.marksFor(account.id) + (
                    folder to mark.copy(
                        // The **range** that was asked about, not the
                        // lowest that came back: a range that is all
                        // gaps returns nothing, and anchoring on what
                        // returned would ask the same empty question
                        // forever.
                        lowestUid = reached,
                        earlierSpan = EarlierPlan.nextSpan(mark.earlierSpan, rows.size),
                    )
                    ),
            )
            Outcome(account.id, rows.size)
        } catch (e: ImapSession.Failure) {
            session.close()
            Outcome(account.id, 0, AccountConnection.readable(e.message ?: "unknown"))
        } catch (e: Exception) {
            session.close()
            Outcome(account.id, 0, "Could not reach the server")
        }
    }
}
