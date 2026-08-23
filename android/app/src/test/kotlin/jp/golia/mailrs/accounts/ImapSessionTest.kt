package jp.golia.mailrs.accounts

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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

    /**
     * `-FLAGS`, not a `FLAGS` that names what should remain: the second
     * replaces the whole set, so it would quietly clear `\Flagged`,
     * `\Answered` and every keyword the person or another client had
     * put there.
     */
    @Test
    fun `marking unread removes one flag and not the rest`() = runBlocking {
        val (s, script) = session("a1 OK done")
        s.markUnseen(7)
        val line = script.written.first()
        assertTrue(line, line.contains("-FLAGS"))
        assertFalse(line, line.contains("+FLAGS"))
        assertTrue(line, line.contains("UID STORE 7"))
    }

    /** What the server can do is read, not assumed. */
    @Test
    fun `capabilities are read from the reply`() = runBlocking {
        val (s, _) = session(
            "* CAPABILITY IMAP4rev1 MOVE UIDPLUS IDLE",
            "a1 OK done",
        )
        val caps = s.capabilities()
        assertTrue(caps.toString(), "MOVE" in caps)
        assertTrue(caps.toString(), "UIDPLUS" in caps)
        assertFalse(caps.toString(), "CONDSTORE" in caps)
    }

    /** A server that offers MOVE gets one command. */
    @Test
    fun `move is one command where the server has it`() = runBlocking {
        val (s, script) = session("a1 OK moved")
        s.moveTo(7, "Trash", setOf("MOVE", "UIDPLUS"))
        assertEquals(1, script.written.size)
        assertTrue(script.written[0], script.written[0].contains("UID MOVE 7 \"Trash\""))
    }

    /**
     * And one that does not gets the older dance — with `UID EXPUNGE`,
     * because **a bare `EXPUNGE` removes every message in the folder
     * flagged `\Deleted`**, including ones another client flagged and
     * has not expunged yet.
     */
    @Test
    fun `without move it copies flags and expunges only the one named`() = runBlocking {
        val (s, script) = session("a1 OK copied", "a2 OK stored", "a3 OK expunged")
        s.moveTo(7, "Trash", setOf("UIDPLUS"))
        assertTrue(script.written[0], script.written[0].contains("UID COPY 7 \"Trash\""))
        assertTrue(script.written[1], script.written[1].contains("+FLAGS (\\Deleted)"))
        assertTrue(script.written[2], script.written[2].contains("UID EXPUNGE 7"))
        assertFalse(
            "a bare EXPUNGE would take other messages with it",
            script.written.any { it.trim().endsWith("EXPUNGE") },
        )
    }

    /**
     * Without UIDPLUS either, the message is flagged and **left**. It
     * disappears from the list either way, and no other message is
     * taken with it.
     */
    @Test
    fun `without uidplus nothing is expunged at all`() = runBlocking {
        val (s, script) = session("a1 OK copied", "a2 OK stored")
        s.moveTo(7, "Trash", emptySet())
        assertEquals(2, script.written.size)
        assertFalse(
            script.written.toString(),
            script.written.any { it.contains("EXPUNGE") },
        )
    }
}
