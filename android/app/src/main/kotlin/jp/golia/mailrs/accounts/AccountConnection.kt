package jp.golia.mailrs.accounts

/**
 * Proving an account works, before anything is stored.
 *
 * A credential that is saved and then found to be wrong is an account
 * that sits in the list doing nothing, and the person has no way to
 * tell that from "there is no new mail". Connecting first turns that
 * into a sentence on the screen they were already looking at.
 *
 * Both halves are checked. A password that reads mail but cannot send
 * is a real thing — providers hand out separate app passwords, and
 * some accept one for IMAP and refuse it for submission — and finding
 * that out at the moment somebody presses Send is the worst time.
 */
object AccountConnection {
    /** What went wrong, in words somebody can act on. */
    data class Failure(
        val stage: Stage,
        val message: String,
        /** Whether re-typing the secret could fix it. */
        val credential: Boolean,
    ) {
        enum class Stage(val label: String) {
            INCOMING("incoming server"),
            OUTGOING("outgoing server"),
        }
    }

    suspend fun verify(account: MailAccount, secret: String): Failure? {
        // The kind decides who is asked. Verifying a POP3 account by
        // talking IMAP to it fails for a reason that has nothing to do
        // with the credential — and the person is told their password
        // is wrong.
        when (account.incoming) {
            Incoming.POP3 -> return verifyPop3(account, secret)
            Incoming.JMAP -> return verifyJmap(account, secret)
            Incoming.IMAP -> Unit
        }
        ImapSession(account.imapHost, account.imapPort).use { imap ->
            try {
                imap.connect()
                if (account.auth == MailProvider.AuthKind.OAUTH2) {
                    imap.authenticateXOAuth2(account.loginName, secret)
                } else {
                    imap.login(account.loginName, secret)
                }
                // Not just the login: a server may accept a credential
                // and then refuse to list, and an account that cannot
                // list is an account with no folders to read.
                imap.list()
            } catch (e: ImapSession.Failure) {
                return failure(Failure.Stage.INCOMING, e)
            } catch (e: Exception) {
                return Failure(Failure.Stage.INCOMING, e.message.orEmpty(), false)
            }
        }
        return verifySmtp(account, secret)
    }

    private fun failure(stage: Failure.Stage, e: ImapSession.Failure): Failure = when (e) {
        is ImapSession.Failure.Refused -> Failure(stage, readable(e.detail), true)
        is ImapSession.Failure.Server -> Failure(stage, readable(e.detail), false)
        is ImapSession.Failure.Unreachable -> Failure(stage, e.why, false)
        is ImapSession.Failure.Closed -> Failure(stage, e.message.orEmpty(), false)
    }

    private fun failure(stage: Failure.Stage, e: SmtpSession.Failure): Failure = when (e) {
        is SmtpSession.Failure.Refused -> Failure(stage, readable(e.detail), true)
        is SmtpSession.Failure.Rejected -> Failure(stage, readable(e.text), false)
        is SmtpSession.Failure.Unreachable -> Failure(stage, e.why, false)
        is SmtpSession.Failure.Closed -> Failure(stage, e.message.orEmpty(), false)
    }

    /**
     * Strip the response code a server puts in front of its reason.
     *
     * `[AUTHENTICATIONFAILED] Invalid credentials` reads better as
     * `Invalid credentials`: the bracket is for programs.
     */
    fun readable(detail: String): String {
        var s = detail.trim()
        if (s.startsWith("[")) {
            val close = s.indexOf(']')
            if (close > 0) s = s.substring(close + 1).trim()
        }
        // And the SMTP enhanced status code, which is the same idea.
        // Three dot-separated numbers, not four: `1.2.3.4` is an
        // address, and stripping it would eat the first word.
        val head = s.substringBefore(' ')
        if (head != s && head.count { it == '.' } == 2 &&
            head.all { it.isDigit() || it == '.' }
        ) {
            s = s.substringAfter(' ')
        }
        return s.ifEmpty { detail }
    }

    /**
     * A POP3 account, checked as far as it can be.
     *
     * The listing as well as the login, for the same reason IMAP's
     * check lists folders: a server may accept a credential and then
     * refuse to say what is in the mailbox, and an account that cannot
     * list is an account with nothing to show.
     *
     * And `QUIT`, because the check itself holds the exclusive lock
     * every POP3 session holds — one left open makes the mailbox
     * unreadable everywhere else until it times out.
     */
    private suspend fun verifyPop3(account: MailAccount, secret: String): Failure? {
        val pop = Pop3Session(account.imapHost, account.imapPort)
        try {
            pop.connect()
            pop.login(account.loginName, secret)
            pop.uidls()
            pop.quit()
        } catch (e: Pop3Session.Failure) {
            return when (e) {
                is Pop3Session.Failure.Refused ->
                    Failure(Failure.Stage.INCOMING, readable(e.detail), true)
                is Pop3Session.Failure.Server ->
                    Failure(Failure.Stage.INCOMING, readable(e.detail), false)
                is Pop3Session.Failure.Unreachable ->
                    Failure(Failure.Stage.INCOMING, readable(e.why), false)
                is Pop3Session.Failure.Closed ->
                    Failure(Failure.Stage.INCOMING, "the server closed the connection", false)
            }
        } finally {
            pop.close()
        }
        return verifySmtp(account, secret)
    }

    /**
     * A JMAP account: the session object, then one real request.
     *
     * Not just the session — a server will hand out `/.well-known/jmap`
     * to anybody, so reading it proves nothing about the credential.
     */
    private suspend fun verifyJmap(account: MailAccount, secret: String): Failure? {
        val user = when (account.auth) {
            MailProvider.AuthKind.OAUTH2 -> ""
            else -> account.loginName
        }
        val jmap = JmapSession(account.imapHost)
        return try {
            val session = jmap.session(user, secret)
            jmap.newest(session, user, secret, 1)
            // No SMTP: JMAP submits over the same API, so the account
            // that reads is the account that sends.
            null
        } catch (e: JmapSession.Failure) {
            val credential = e is JmapSession.Failure.Refused
            Failure(Failure.Stage.INCOMING, readable(e.message.orEmpty()), credential)
        } catch (e: Exception) {
            Failure(Failure.Stage.INCOMING, readable(e.message.orEmpty()), false)
        }
    }

    /** The outgoing half, shared by the kinds that have one. */
    /**
     * The outgoing half, shared by the kinds that have one.
     *
     * Greeted with the name real mail will be greeted with. Checking
     * with `localhost` and sending with the address's domain is two
     * different conversations, and a server that greylists on the
     * greeting will pass one and hold the other.
     */
    private suspend fun verifySmtp(account: MailAccount, secret: String): Failure? {
        SmtpSession(account.smtpHost, account.smtpPort).use { smtp ->
            try {
                smtp.connect(AccountSender.helo(account))
                smtp.authenticate(
                    account.loginName,
                    secret,
                    account.auth == MailProvider.AuthKind.OAUTH2,
                )
            } catch (e: SmtpSession.Failure) {
                return failure(Failure.Stage.OUTGOING, e)
            } catch (e: Exception) {
                return Failure(Failure.Stage.OUTGOING, e.message.orEmpty(), false)
            }
        }
        return null
    }
}
