package jp.golia.mailrs.accounts

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * The SMTP conversation, without a server.
 *
 * A real server is exactly the thing that will not send the awkward
 * case on demand — a multi-line greeting, a 334 refusal of an OAuth
 * token, a 4xx when the queue is full. A scripted one sends them every
 * time.
 */
class SmtpSessionTest {
    private class Script(private val lines: MutableList<String>) : SmtpSession.Transport {
        val written = mutableListOf<String>()
        override fun readLine(): String {
            if (lines.isEmpty()) throw SmtpSession.Failure.Closed()
            return lines.removeAt(0)
        }
        override fun write(text: String) {
            written.add(text)
        }
        override fun close() = Unit
    }

    private fun session(vararg lines: String): Pair<SmtpSession, Script> {
        val script = Script(lines.toMutableList())
        val s = SmtpSession("localhost", 587)
        s.transport = script
        return s to script
    }

    /**
     * The **fourth character** says whether more is coming — `250-` is a
     * continuation and `250 ` is the end. A parser that reads the code
     * alone stops at the first line of every EHLO and then reads the
     * capability list as the answer to the next command.
     */
    @Test
    fun `a multi line reply is read to its end`() = runBlocking {
        val (s, script) = session(
            "220 smtp.example.com ESMTP",
            "250-smtp.example.com greets you",
            "250-SIZE 35882577",
            "250-AUTH LOGIN PLAIN XOAUTH2",
            "250 STARTTLS",
            "220 2.0.0 Ready to start TLS",
            "250-smtp.example.com greets you again",
            "250 STARTTLS",
            "250 2.1.0 sender ok",
            "250 2.1.5 recipient ok",
            "354 go ahead",
            "250 2.0.0 queued",
            "221 bye",
        )
        s.connect("example.com")
        // If EHLO had been read short, this MAIL FROM would consume one
        // of the capability lines instead of its own reply, and the
        // whole exchange would slide by one.
        s.send("me@example.com", listOf("you@example.com"), "Subject: x\r\n\r\nhi\r\n")
        assertTrue(script.written.any { it.startsWith("EHLO example.com") })
        assertTrue(script.written.any { it.startsWith("MAIL FROM:<me@example.com>") })
    }

    /**
     * `AUTH PLAIN` sends an empty authorisation identity, then the
     * login, then the secret, \u0000-separated. Sending the login in the
     * first field as well — the obvious misreading — is refused by some
     * servers and silently accepted as a different user by others.
     */
    @Test
    fun `auth plain separates with nul and leads with nothing`() = runBlocking {
        val (s, script) = session("235 2.7.0 Accepted")
        s.authenticate("me@example.com", "secret", false)
        val line = script.written.first { it.startsWith("AUTH PLAIN") }
        val payload = line.removePrefix("AUTH PLAIN ").trim()
        val decoded = String(java.util.Base64.getDecoder().decode(payload), Charsets.UTF_8)
        assertEquals("\u0000me@example.com\u0000secret", decoded)
    }

    /**
     * A provider that rejects an OAuth token answers **334** with a
     * base64 error rather than a final code. Reading the 334 as success
     * authenticates every refused token.
     */
    @Test
    fun `a 334 is a refusal and not a success`() = runBlocking {
        val (s, _) = session(
            "334 eyJzdGF0dXMiOiI0MDEifQ==",
            "535 5.7.8 Username and Password not accepted",
        )
        try {
            s.authenticate("me@example.com", "token", true)
            fail("a refused token was reported as signed in")
        } catch (e: SmtpSession.Failure.Refused) {
            assertTrue(e.detail, e.detail.contains("not accepted"))
        }
    }

    /**
     * A body line beginning with `.` would end the DATA block. Left
     * unstuffed, the message arrives cut in half — and the half that
     * arrives looks like a whole message.
     */
    @Test
    fun `a body line of a single dot is stuffed`() = runBlocking {
        val (s, script) = session(
            "250 2.1.0 sender ok",
            "250 2.1.5 recipient ok",
            "354 go ahead",
            "250 2.0.0 queued",
            "221 bye",
        )
        s.send("me@a.com", listOf("you@b.com"), "Subject: x\r\n\r\n.\r\nnot the end\r\n")
        val data = script.written.first { it.contains("not the end") }
        assertTrue(data, data.contains("\r\n..\r\n"))
        assertTrue("the block was never terminated", data.endsWith("\r\n.\r\n"))
    }

    /**
     * 4xx is the moment's fault and 5xx is the message's, and the
     * session must carry which — an "is it worth trying again" that gets
     * this wrong retries forever or gives up at once.
     */
    @Test
    fun `a temporary rejection is marked temporary`() = runBlocking {
        val (s, _) = session("451 4.3.0 Try again later")
        try {
            s.send("me@a.com", listOf("you@b.com"), "x")
            fail("a rejected MAIL FROM was reported as sent")
        } catch (e: SmtpSession.Failure.Rejected) {
            assertEquals(451, e.code)
            assertFalse(e.permanent)
        }
    }

    /**
     * **No downgrade.** A server that does not offer to encrypt is not
     * argued with — the credential simply does not go there. A stripped
     * capability list is what an attacker in the middle produces, and
     * the only defence is refusing rather than asking.
     */
    @Test
    fun `a server that will not encrypt gets no credential`() = runBlocking {
        val (s, script) = session(
            "220 smtp.example.com ESMTP",
            "250-smtp.example.com greets you",
            "250 SIZE 35882577",
        )
        try {
            s.connect("example.com")
            fail("an unencrypted submission was allowed to continue")
        } catch (e: SmtpSession.Failure.Refused) {
            assertTrue(e.detail, e.detail.contains("encrypt"))
        }
        assertFalse(
            "STARTTLS was asked for after the server said it could not",
            script.written.any { it.startsWith("STARTTLS") },
        )
    }

    /** And one that offers it and then refuses is dropped too. */
    @Test
    fun `a refused upgrade is not continued in the clear`() = runBlocking {
        val (s, _) = session(
            "220 smtp.example.com ESMTP",
            "250-smtp.example.com greets you",
            "250 STARTTLS",
            "454 4.7.0 TLS not available",
        )
        try {
            s.connect("example.com")
            fail("a refused upgrade was continued in the clear")
        } catch (e: SmtpSession.Failure.Refused) {
            assertTrue(e.detail, e.detail.contains("encrypt"))
        }
    }

    /**
     * 465 is encrypted from the first byte, so it neither offers nor
     * needs STARTTLS — asking for it there is a command the server has
     * every right to refuse.
     */
    @Test
    fun `implicit tls does not ask to upgrade`() = runBlocking {
        val script = Script(mutableListOf("220 ready", "250 smtp.example.com").toMutableList())
        val s = SmtpSession("localhost", 465)
        s.transport = script
        s.connect("example.com")
        assertFalse(script.written.any { it.startsWith("STARTTLS") })
    }

    /** Every recipient is offered, and one refusal is not silent. */
    @Test
    fun `each recipient is named`() = runBlocking {
        val (s, script) = session("250 ok", "250 ok", "550 no such user")
        try {
            s.send("me@a.com", listOf("a@b.com", "c@d.com"), "x")
            fail("a refused recipient was reported as sent")
        } catch (e: SmtpSession.Failure.Rejected) {
            assertEquals(550, e.code)
        }
        assertTrue(script.written.any { it.startsWith("RCPT TO:<a@b.com>") })
        assertTrue(script.written.any { it.startsWith("RCPT TO:<c@d.com>") })
    }
}
