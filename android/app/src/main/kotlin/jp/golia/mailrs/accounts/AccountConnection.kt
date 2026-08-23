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

    suspend fun verify(
        account: MailAccount,
        secret: String,
        helo: String = "localhost",
    ): Failure? {
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
        SmtpSession(account.smtpHost, account.smtpPort).use { smtp ->
            try {
                smtp.connect(helo)
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
}
