package jp.golia.mailrs.accounts

import java.util.TimeZone
import java.util.UUID

/**
 * Sending through a connected account.
 *
 * What goes on the wire is [OutgoingMessage], which is pure and tested.
 * This opens the socket and reports what came back in words a person can
 * act on — a rejection from a mail server is a number and a sentence
 * written for another machine.
 */
object AccountSender {
    /**
     * How a session is made.
     *
     * Injectable so the builder and the wire can be checked together.
     * Both halves are tested apart, and the seam between them is where
     * **a Bcc would leak** — the address belongs in `RCPT TO` and
     * nowhere in the DATA block, and only an end-to-end look can say
     * that it is so.
     */
    internal var openSmtp: (String, Int) -> SmtpSession = { host, port -> SmtpSession(host, port) }
    sealed class Outcome {
        object Sent : Outcome()
        data class Failed(val why: String) : Outcome()
    }

    suspend fun send(
        draft: OutgoingMessage.Draft,
        account: MailAccount,
        store: AccountStore,
        bcc: List<String> = emptyList(),
        nowSeconds: Long = System.currentTimeMillis() / 1000,
    ): Outcome {
        val recipients = OutgoingMessage.envelope(draft, bcc)
        if (recipients.isEmpty()) return Outcome.Failed("Add somebody to send this to")
        // Refused before sending rather than discovered during it: a
        // message stopped here is a message somebody still has, and one
        // that dies mid-send looks exactly like mail that vanished.
        val room = OutgoingLimits.check(draft)
        if (room is OutgoingLimits.Verdict.TooLarge) {
            return Outcome.Failed(
                "Too large to send: " + humanBytes(room.attachedBytes) +
                    " attached, and about " + humanBytes(room.limitBytes) + " is the most.",
            )
        }
        val secret = store.secret(account.id)
            ?: return Outcome.Failed("Sign in again to send from this account")
        val message = OutgoingMessage.text(
            draft, identity(account), nowSeconds, TimeZone.getDefault(),
        )
        val session = openSmtp(account.smtpHost, account.smtpPort)
        return try {
            // The domain of the address, not the device's name: a HELO
            // naming somebody's phone is refused by a fair number of
            // servers and greylisted by more.
            session.connect(helo(account))
            session.authenticate(
                account.loginName, secret, account.auth == MailProvider.AuthKind.OAUTH2,
            )
            // The envelope sender is the account's own address. A server
            // that permits one address will refuse another, and SPF
            // makes that refusal correct.
            session.send(account.address, recipients, message)
            session.close()
            Outcome.Sent
        } catch (e: SmtpSession.Failure) {
            session.close()
            Outcome.Failed(explain(e))
        } catch (e: Exception) {
            session.close()
            Outcome.Failed("Could not reach the outgoing server")
        }
    }

    /**
     * A Message-ID nobody else will mint.
     *
     * The domain half is the account's own, because a Message-ID
     * pointing at a domain that has nothing to do with the sender is one
     * of the things spam filters count.
     */
    fun identity(account: MailAccount, uuid: String = UUID.randomUUID().toString()): String =
        uuid.lowercase() + "@" + domainOf(account.address)

    fun helo(account: MailAccount): String {
        val host = domainOf(account.address)
        return when {
            host.isEmpty() -> "localhost"
            else -> host
        }
    }

    private fun domainOf(address: String): String {
        val parts = address.split("@")
        return when (parts.size) {
            2 -> parts[1].lowercase()
            else -> ""
        }
    }

    /** A server's refusal, in words somebody can act on. */
    fun explain(e: SmtpSession.Failure): String = when (e) {
        is SmtpSession.Failure.Rejected -> when {
            // 5xx is the message's fault and 4xx is the moment's; a
            // person told "try again" about a permanent rejection will
            // try again forever.
            !e.permanent -> "The server is busy — try again shortly (${e.code})"
            e.code == 550 || e.code == 553 ->
                "The server refused the recipient or the sender address (${e.code}): ${e.text}"
            e.code == 535 -> "The server refused the sign-in for this account (535)"
            else -> "The server refused this message (${e.code}): ${e.text}"
        }
        is SmtpSession.Failure.Refused -> AccountConnection.readable(e.detail)
        is SmtpSession.Failure.Unreachable -> AccountConnection.readable(e.why)
        is SmtpSession.Failure.Closed -> "The outgoing server closed the connection"
    }

    /** Bytes in the units somebody chose the files in. */
    internal fun humanBytes(bytes: Long): String = when {
        bytes < 1_000 -> "$bytes B"
        bytes < 1_000_000 -> String.format(java.util.Locale.US, "%.0f KB", bytes / 1_000.0)
        else -> String.format(java.util.Locale.US, "%.0f MB", bytes / 1_000_000.0)
    }
}
