package jp.golia.mailrs.accounts

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * The IMAP conversation, without a server.
 *
 * Every rule here is a rule about what arrives, and none of them can be
 * checked by connecting to a real server and hoping it sends the
 * awkward case. A scripted transport sends the awkward case every time.
 */
class ImapSessionTest {
    /** A server that says exactly what it is told to say. */
    private class Script(private val lines: MutableList<String>) : ImapSession.Transport {
        val written = mutableListOf<String>()
        private var pending = StringBuilder()

        override fun readLine(): String {
            if (pending.isNotEmpty()) {
                val out = pending.toString()
                pending = StringBuilder()
                return out
            }
            if (lines.isEmpty()) throw ImapSession.Failure.Closed()
            return lines.removeAt(0)
        }

        override fun readBytes(count: Int): String {
            val out = StringBuilder()
            while (out.length < count) {
                if (lines.isEmpty()) throw ImapSession.Failure.Closed()
                out.append(lines.removeAt(0))
            }
            // Whatever the literal did not use belongs to the next line,
            // exactly as it would on a socket.
            if (out.length > count) pending = StringBuilder(out.substring(count))
            return out.substring(0, count)
        }

        override fun write(text: String) {
            written.add(text.trimEnd('\r', '\n'))
        }

        override fun close() = Unit
    }

    private fun session(vararg lines: String): Pair<ImapSession, Script> {
        val script = Script(lines.toMutableList())
        val s = ImapSession("localhost", 993)
        s.transport = script
        return s to script
    }

    /**
     * `a1` must not be completed by `a10`'s reply. A prefix match here
     * ends the wrong command, and the rest of that command's output is
     * read as the next one's.
     */
    @Test
    fun `a tag is matched whole`() = runBlocking {
        val (s, script) = session(
            "* LIST (\\HasNoChildren) \".\" \"INBOX\"",
            "a1 OK done",
        )
        val folders = s.list()
        assertEquals(listOf("INBOX"), folders.map { it.name })
        assertTrue(script.written.first(), script.written.first().startsWith("a1 LIST"))
    }

    /** A folder name may contain spaces, and it is at the end of the line. */
    @Test
    fun `a folder name with spaces survives`() = runBlocking {
        val (s, _) = session(
            "* LIST (\\HasNoChildren) \"/\" \"[Gmail]/All Mail\"",
            "a1 OK done",
        )
        assertEquals(listOf("[Gmail]/All Mail"), s.list().map { it.name })
    }

    /** SELECT reports where the folder is, and both numbers matter. */
    @Test
    fun `select reads uidvalidity and exists`() = runBlocking {
        val (s, _) = session(
            "* 42 EXISTS",
            "* OK [UIDVALIDITY 1234567890] UIDs valid",
            "a1 OK [READ-WRITE] SELECT completed",
        )
        val (validity, exists) = s.select("INBOX")
        assertEquals(1234567890L, validity)
        assertEquals(42, exists)
    }

    /**
     * A literal is read by the byte count the server announced, never by
     * scanning for a terminator — a message contains every byte sequence
     * a terminator could be made of, including `)` and a bare `.`.
     */
    @Test
    fun `a literal is read by its announced length`() = runBlocking {
        val body = "Subject: has a ) in it\r\nFrom: a@b\r\n\r\n"
        val (s, _) = session(
            "* 1 FETCH (UID 7 FLAGS (\\Seen) BODY[HEADER] {${body.length}}",
            body,
            ")",
            "a1 OK done",
        )
        val fetched = s.fetchHeaders("1:*")
        assertEquals(1, fetched.size)
        assertEquals(7L, fetched[0].uid)
        assertTrue(fetched[0].seen)
        assertEquals("has a ) in it", fetched[0].headers.subject)
    }

    /** An unread message is unread, and the flag list says so by absence. */
    @Test
    fun `absence of the seen flag means unread`() = runBlocking {
        val body = "Subject: x\r\n\r\n"
        val (s, _) = session(
            "* 1 FETCH (UID 8 FLAGS () BODY[HEADER] {${body.length}}",
            body,
            ")",
            "a1 OK done",
        )
        assertEquals(false, s.fetchHeaders("1:*")[0].seen)
    }

    /**
     * `BODY.PEEK[]`, never `BODY[]`: opening a message is the reader's
     * decision, and a client that marks mail read for having looked at
     * it takes that decision away.
     */
    @Test
    fun `fetching a body peeks`() = runBlocking {
        val raw = "Subject: hi\r\n\r\nbody text\r\n"
        val (s, script) = session(
            "* 1 FETCH (UID 9 BODY[] {${raw.length}}",
            raw,
            ")",
            "a1 OK done",
        )
        assertEquals(raw, String(s.fetchRaw(9), Charsets.ISO_8859_1))
        assertTrue(script.written.first(), script.written.first().contains("BODY.PEEK[]"))
    }

    /** A refusal is a refusal, and it carries what the server said. */
    @Test
    fun `a no reply becomes a failure carrying its reason`() = runBlocking {
        val (s, _) = session("a1 NO [AUTHENTICATIONFAILED] Invalid credentials")
        try {
            s.login("me", "wrong")
            fail("a refused login was reported as success")
        } catch (e: ImapSession.Failure.Refused) {
            assertTrue(e.detail, e.detail.contains("Invalid credentials"))
        }
    }

    /**
     * A password with a quote in it is quoted rather than sent raw.
     * Generated app passwords contain `"` and `\` often enough that an
     * unquoted LOGIN turns one into a syntax error — and the person is
     * told their password is wrong when it is right.
     */
    @Test
    fun `a password with a quote is escaped`() = runBlocking {
        val (s, script) = session("a1 OK signed in")
        s.login("me", "pa\"ss\\word")
        val line = script.written.first()
        assertTrue(line, line.contains("\\\"") && line.contains("\\\\"))
    }
}
