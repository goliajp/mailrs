package jp.golia.mailrs.accounts

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Keeping one IMAP connection per account.
 *
 * The interesting assertion is not that a second action skips the
 * handshake — that one is easy and would pass on a pool with the
 * defect. It is that a connection the server has since dropped does
 * not surface as a failed action, because that is what a naive pool
 * ships and what nobody sees while they are looking.
 */
class ImapPoolTest {
    /** A server that answers a greeting, a login, and then NOOPs. */
    private class Script(
        private val lines: MutableList<String>,
        /** Set to make the connection behave as one the server dropped. */
        var dead: Boolean = false,
    ) : ImapSession.Transport {
        val written = mutableListOf<String>()
        var closed = 0

        override fun readLine(): String {
            if (dead) throw ImapSession.Failure.Closed()
            if (lines.isEmpty()) throw ImapSession.Failure.Closed()
            return lines.removeAt(0)
        }

        override fun readBytes(count: Int): String = ""

        override fun write(text: String) {
            if (dead) throw ImapSession.Failure.Closed()
            written.add(text.trimEnd('\r', '\n'))
        }

        override fun close() {
            closed += 1
        }
    }

    private val account = MailAccount(
        id = "a", address = "me@x.jp", login = "me@x.jp",
        imapHost = "imap.x.jp", imapPort = 993,
        smtpHost = "smtp.x.jp", smtpPort = 465,
    )

    /** Greeting, LOGIN ok, then an OK for every command asked. */
    private fun script(vararg extra: String) = Script(
        (listOf("* OK ready", "a1 OK logged in") + extra).toMutableList(),
    )

    private fun pool(scripts: List<Script>, clock: () -> Long = { 0L }): Pair<ImapPool, MutableList<Script>> {
        val handed = mutableListOf<Script>()
        val remaining = scripts.toMutableList()
        val pool = ImapPool(
            open = { _, _ ->
                val next = remaining.removeAt(0)
                handed.add(next)
                ImapSession("h", 993).also { it.transport = next }
            },
            now = clock,
        )
        return pool to handed
    }

    // Two actions in a row are one connection: the second pays no TLS
    // handshake and no LOGIN, which is the whole point.
    @Test
    fun `a second action reuses the connection`() = runBlocking {
        val first = script("a2 OK done", "a3 OK done")
        val (pool, handed) = pool(listOf(first, script()))
        pool.use(account, "pw") { it.capabilities() }
        pool.use(account, "pw") { it.capabilities() }
        assertEquals("a second connection was opened", 1, handed.size)
        assertEquals("logged in twice", 1, first.written.count { it.contains("LOGIN") })
    }

    // **The one that matters.** A connection kept past the freshness
    // window and dropped by the server must be replaced, not reported.
    // Nothing about this is visible in the moment it is created — it
    // only appears after a pause, which is to say when nobody is
    // watching.
    @Test
    fun `a connection the server dropped is replaced, not reported`() = runBlocking {
        val stale = script()
        val fresh = script("a2 OK done")
        var clock = 0L
        val (pool, handed) = pool(listOf(stale, fresh)) { clock }
        pool.use(account, "pw") { }
        // Time passes, and the server hangs up while nothing is happening.
        clock = ImapPool.FRESH_MS + 1
        stale.dead = true
        val answered = pool.use(account, "pw") { it.capabilities(); "done" }
        assertEquals("the action failed on a server that was fine", "done", answered)
        assertEquals("the dead connection was not replaced", 2, handed.size)
        assertTrue("the dead socket was left open", stale.closed > 0)
    }

    // Inside the window the probe is skipped — a person filing a run of
    // messages should not pay a round trip between each.
    @Test
    fun `a connection used moments ago is not probed`() = runBlocking {
        val only = script("a2 OK done")
        val (pool, _) = pool(listOf(only, script())) { 0L }
        pool.use(account, "pw") { }
        pool.use(account, "pw") { }
        assertFalse("a NOOP was sent inside the freshness window",
            only.written.any { it.contains("NOOP") })
    }

    @Test
    fun `beyond the window the connection is probed`() = runBlocking {
        val only = script("a2 OK noop done")
        var clock = 0L
        val (pool, _) = pool(listOf(only, script())) { clock }
        pool.use(account, "pw") { }
        clock = ImapPool.FRESH_MS + 1
        pool.use(account, "pw") { }
        assertTrue("the connection was handed out unasked",
            only.written.any { it.contains("NOOP") })
    }

    // A session that threw may have an answer still in flight, and the
    // next caller would read it as its own.
    @Test
    fun `a session that failed is not handed out again`() = runBlocking {
        val broken = script()
        val (pool, handed) = pool(listOf(broken, script("a2 OK done")))
        pool.use(account, "pw") { }
        broken.dead = true
        runCatching { pool.use(account, "pw") { it.capabilities() } }
        // Freshly opened for the third call rather than reused.
        pool.use(account, "pw") { }
        assertEquals("the failed session was handed out again", 2, handed.size)
        assertEquals("more than one connection survived", 1, pool.size())
        assertTrue("the failed socket was left open", broken.closed > 0)
    }

    // A credential that has just been deleted must not leave a socket
    // open that is still signed in with it.
    @Test
    fun `dropping an account closes its connection`() = runBlocking {
        val only = script()
        val (pool, _) = pool(listOf(only, script()))
        pool.use(account, "pw") { }
        pool.drop(account.id)
        assertTrue("the socket was left open", only.closed > 0)
        assertEquals(0, pool.size())
    }

    // Two accounts are two connections: a server caps them per account,
    // and one shared socket would be signed in as the wrong person.
    @Test
    fun `each account gets its own connection`() = runBlocking {
        val (pool, handed) = pool(listOf(script(), script()))
        pool.use(account, "pw") { }
        pool.use(account.copy(id = "b", address = "you@x.jp"), "pw") { }
        assertEquals(2, handed.size)
        assertEquals(2, pool.size())
    }

    // An account removed **while an action is running**. The first
    // version of this pool took the connection out of the table for
    // the length of the call, so `drop` found nothing to close and the
    // action then put it back — leaving a socket open and signed in
    // with a credential that had just been deleted.
    @Test
    fun `an account dropped mid-action does not come back`() = runBlocking {
        val only = script("a2 OK done")
        val (pool, _) = pool(listOf(only, script()))
        pool.use(account, "pw") {
            pool.drop(account.id)
            it.capabilities()
        }
        assertEquals("the dropped connection was put back", 0, pool.size())
        assertTrue("the dropped socket was left open", only.closed > 0)
    }
}
