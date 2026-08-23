package jp.golia.mailrs.accounts

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/** The POP3 conversation, without a server. */
class Pop3SessionTest {
    private class Script(private val lines: MutableList<String>) : Pop3Session.Transport {
        val written = mutableListOf<String>()
        override fun readLine(): String {
            if (lines.isEmpty()) throw Pop3Session.Failure.Closed()
            return lines.removeAt(0)
        }
        override fun write(text: String) {
            written.add(text.trimEnd('\r', '\n'))
        }
        override fun close() = Unit
    }

    private fun session(vararg lines: String): Pair<Pop3Session, Script> {
        val script = Script(lines.toMutableList())
        val s = Pop3Session("localhost", 995)
        s.transport = script
        return s to script
    }

    /**
     * **Both** answers matter. A server that accepts the name and
     * refuses the password says so only on the second, and a client
     * that checks one of them signs in to nothing.
     */
    @Test
    fun `a refused password is a refusal even after an accepted user`() = runBlocking {
        val (s, _) = session("+OK user accepted", "-ERR [AUTH] Invalid password")
        try {
            s.login("me", "wrong")
            fail("a refused password was reported as signed in")
        } catch (e: Pop3Session.Failure.Refused) {
            assertTrue(e.detail, e.detail.contains("Invalid password"))
        }
    }

    /**
     * POP3 has no code for "wrong credential", so the words are all
     * there is — and a refusal that is not about the credential must
     * not be reported as one, or somebody retypes a password that was
     * always right.
     */
    @Test
    fun `a refusal that is not about the credential is not one`() = runBlocking {
        val (s, _) = session("+OK", "-ERR mailbox locked by another session")
        try {
            s.login("me", "right")
            fail("a locked mailbox was reported as signed in")
        } catch (e: Pop3Session.Failure.Server) {
            assertTrue(e.detail, e.detail.contains("locked"))
        }
    }

    /**
     * `UIDL` is the only durable identity POP3 offers: message numbers
     * are renumbered every session, so a client that remembers numbers
     * re-downloads the mailbox after any delete made elsewhere.
     */
    @Test
    fun `the listing is read to its terminator`() = runBlocking {
        val (s, _) = session(
            "+OK",
            "1 QhdPYR:00WBw1Ph7x7",
            "2 QhdPYR:00WBw1Ph7x8",
            ".",
        )
        val all = s.uidls()
        assertEquals(listOf(1, 2), all.map { it.number })
        assertEquals("QhdPYR:00WBw1Ph7x7", all[0].id)
    }

    /**
     * `TOP n 0` — headers and none of the body. Fetching whole messages
     * to show a list downloads the mailbox to display it, and on a phone
     * that is somebody's data allowance.
     */
    @Test
    fun `headers are asked for without the body`() = runBlocking {
        val (s, script) = session(
            "+OK",
            "Subject: hi",
            "From: a@b",
            "",
            ".",
        )
        val head = s.headers(3)
        assertEquals("TOP 3 0", script.written.first())
        assertTrue(head, head.contains("Subject: hi"))
    }

    /**
     * A body line that began with `.` arrives doubled. A client that
     * does not undo it corrupts every message containing such a line —
     * and `.` alone ends the response and is not part of the message.
     */
    @Test
    fun `dot stuffing is undone`() = runBlocking {
        val (s, _) = session(
            "+OK",
            "Subject: x",
            "",
            "..hidden dot",
            "ordinary",
            ".",
        )
        val message = String(s.retrieve(1), Charsets.ISO_8859_1)
        assertTrue(message, message.contains("\r\n.hidden dot\r\n"))
        assertTrue(message, message.endsWith("ordinary"))
    }

    /**
     * A POP3 server holds an exclusive lock on the mailbox for the
     * length of a session. One dropped without QUIT keeps that lock
     * until it times out, during which nothing else — including the
     * person's other device — can read their mail.
     */
    @Test
    fun `the session is ended properly`() = runBlocking {
        val (s, script) = session("+OK bye")
        s.quit()
        assertTrue(script.written.any { it == "QUIT" })
    }

    /** A greeting that is not a greeting is not a connection. */
    @Test
    fun `a refused connection is not a connection`() = runBlocking {
        val (s, _) = session("-ERR server busy, try later")
        try {
            s.connect()
            fail("a refused connection was reported as open")
        } catch (e: Pop3Session.Failure.Server) {
            assertTrue(e.detail, e.detail.contains("busy"))
        }
    }
}
