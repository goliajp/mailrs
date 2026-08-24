package jp.golia.mailrs.accounts

import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * One live IMAP connection per account, reused between actions.
 *
 * Every tap used to be its own connection: TCP, TLS, `LOGIN`, the one
 * command it came for, `LOGOUT`. On a phone that is a second or two
 * before anything happens, for a command that takes milliseconds — and
 * a person filing ten messages paid it ten times.
 *
 * **One connection, not a pool of several.** IMAP servers cap
 * concurrent connections per account (Gmail allows fifteen, and counts
 * every device), and a phone is doing one thing at a time. A second
 * connection would buy nothing and spend somebody's quota.
 *
 * ### The part that is not about speed
 *
 * A kept connection may already be dead. Servers drop idle ones, NATs
 * drop them sooner, and a socket gives no sign of it until something
 * is written. A pool that does not account for this ships a specific
 * defect: **the first tap after a while fails, and says the server
 * could not be reached about a server that is fine.** Worse, it is
 * intermittent by construction — it happens after a pause, so it never
 * happens while anybody is looking.
 *
 * So a connection older than [FRESH_MS] is asked `NOOP` before it is
 * handed out, and one that does not answer is replaced.
 *
 * **The action itself is not retried.** A failure part-way through a
 * `MOVE` cannot be told from a failure before it, and repeating it
 * would file the message twice. The probe is what makes retrying
 * unnecessary in the common case; where it is not enough, failing is
 * the honest answer.
 */
class ImapPool(
    internal var open: (String, Int) -> ImapSession = { host, port -> ImapSession(host, port) },
    internal var now: () -> Long = { System.currentTimeMillis() },
) {
    private class Held(val session: ImapSession, var lastUsed: Long)

    private val locks = ConcurrentHashMap<String, Mutex>()
    private val held = ConcurrentHashMap<String, Held>()

    /**
     * Run [body] against a signed-in session for [account].
     *
     * Exclusive for the length of the call: IMAP interleaves untagged
     * responses with tagged ones, so two commands in flight on one
     * connection is a parser that reads somebody else's answer.
     */
    suspend fun <T> use(
        account: MailAccount,
        secret: String,
        body: suspend (ImapSession) -> T,
    ): T = locks.computeIfAbsent(account.id) { Mutex() }.withLock {
        val session = reusable(account) ?: freshly(account, secret).also {
            held[account.id] = Held(it, now())
        }
        try {
            val out = body(session)
            // `replace`, not `put`: [drop] may have run while this was
            // in flight — the account removed, the socket closed under
            // the command — and putting it back would resurrect a
            // connection signed in with a credential that is gone.
            held.replace(account.id, Held(session, now()))
            out
        } catch (e: Throwable) {
            // Not put back. A session that threw may be mid-command,
            // with an answer still to arrive, and the next caller would
            // read it as its own.
            held.remove(account.id)
            runCatching { session.close() }
            throw e
        }
    }

    /** Close and forget this account's connection, if it has one. */
    fun drop(accountId: String) {
        held.remove(accountId)?.let { runCatching { it.session.close() } }
    }

    /**
     * Close and forget every connection.
     *
     * **No production caller, deliberately.** Removing an account
     * closes that account's connection, which is the case that
     * matters; there is no sign-out-everything in the app, and a
     * lifecycle hook to close on backgrounding would be a second
     * answer to a question the freshness probe already answers.
     *
     * It exists so a test can put its pool back before the next one
     * runs. Stated here rather than left to be noticed, because a
     * method nobody calls is usually a feature that is off.
     */
    fun dropAll() {
        for (id in held.keys.toList()) drop(id)
    }

    /** How many connections are being kept. Reads state; changes none. */
    internal fun size() = held.size

    /**
     * The kept connection, if there is one and it answers.
     *
     * Read rather than taken: it stays in [held] for the whole call, so
     * [drop] can still find and close it if the account is removed
     * while this one is in flight.
     */
    private suspend fun reusable(account: MailAccount): ImapSession? {
        val candidate = held[account.id] ?: return null
        if (now() - candidate.lastUsed <= FRESH_MS) return candidate.session
        return try {
            candidate.session.noop()
            candidate.session
        } catch (_: Throwable) {
            held.remove(account.id)
            runCatching { candidate.session.close() }
            null
        }
    }

    private suspend fun freshly(account: MailAccount, secret: String): ImapSession {
        val session = open(account.imapHost, account.imapPort)
        try {
            session.connect()
            when (account.auth) {
                MailProvider.AuthKind.OAUTH2 ->
                    session.authenticateXOAuth2(account.loginName, secret)
                else -> session.login(account.loginName, secret)
            }
        } catch (e: Throwable) {
            runCatching { session.close() }
            throw e
        }
        return session
    }

    companion object {
        /**
         * How recently a connection must have been used to be handed
         * out without asking the server whether it is still there.
         *
         * Short, because the cost of being wrong is a failed action and
         * the cost of being careful is one round trip. Somebody filing
         * a run of messages stays inside it; somebody coming back to
         * the app does not, and pays a `NOOP` instead of a lie.
         */
        const val FRESH_MS = 30_000L

        /** The one pool for this process. */
        val shared = ImapPool()
    }
}
