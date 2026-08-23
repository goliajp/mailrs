package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Reading what a POP3 server says. */
class Pop3LineTest {
    @Test
    fun `ok and err are told apart`() {
        assertEquals(Pop3.Reply(true, "2 messages"), Pop3.reply("+OK 2 messages"))
        assertEquals(Pop3.Reply(false, "bad password"), Pop3.reply("-ERR bad password"))
        assertEquals(Pop3.Reply(true, ""), Pop3.reply("+OK"))
        assertNull(Pop3.reply("nonsense"))
    }

    // Message numbers are renumbered every session; the uidl is the
    // only thing that survives. A client that remembers numbers
    // re-downloads the mailbox after any delete made elsewhere.
    @Test
    fun `a uidl line is a number and an identity`() {
        assertEquals(Pop3.Uidl(3, "QhdPYR:00WBw1Ph7x7"), Pop3.uidl("3 QhdPYR:00WBw1Ph7x7"))
    }

    // A uidl may hold anything printable, including spaces on some
    // servers — so the split is on the **first** space only.
    @Test
    fun `a uidl with a space survives`() {
        assertEquals("abc def", Pop3.uidl("7 abc def")?.id)
    }

    @Test
    fun `a line that is not a uidl is not guessed at`() {
        assertNull(Pop3.uidl("3"))
        assertNull(Pop3.uidl("x abc"))
        assertNull(Pop3.uidl("3 "))
    }

    // The mirror of SMTP's dot-stuffing. A client that does not undo it
    // corrupts every message with a line starting `.`.
    @Test
    fun `dot stuffing is undone`() {
        assertEquals(
            "first\r\n.hidden\r\nlast",
            Pop3.unstuffed(listOf("first", "..hidden", "last", ".")),
        )
    }

    // `.` alone ends the response and is not part of the message.
    @Test
    fun `the terminator is not part of the message`() {
        assertEquals("body", Pop3.unstuffed(listOf("body", ".", "after")))
    }

    // A dot inside a line is not stuffing and must not be eaten.
    @Test
    fun `a dot inside a line is left alone`() {
        assertEquals("see fig. 1", Pop3.unstuffed(listOf("see fig. 1", ".")))
    }

    @Test
    fun `a refused credential is recognised from the words alone`() {
        assertTrue(Pop3.isAuthenticationFailure("authentication failed"))
        assertTrue(Pop3.isAuthenticationFailure("invalid username or password"))
        assertFalse(Pop3.isAuthenticationFailure("server busy, try again later"))
    }
}
