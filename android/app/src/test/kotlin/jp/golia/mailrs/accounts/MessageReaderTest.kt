package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The part of opening a message that needs no server: raw bytes in,
 * something a person can read out.
 */
class MessageReaderTest {
    private fun raw(s: String) = s.toByteArray(Charsets.UTF_8)

    @Test
    fun `plain text arrives as itself`() {
        val out = MessageReader.display(raw("Subject: x\r\n\r\nJust words.\r\n"))
        assertEquals("Just words.\r\n", out.text)
        assertFalse(out.fromHtml)
    }

    /**
     * Markup becomes text rather than being rendered: that is what stops
     * a message asking somebody else's server for an image and reporting
     * that it was read.
     */
    @Test
    fun `markup becomes text`() {
        val message = "Content-Type: text/html; charset=utf-8\r\n\r\n" +
            "<html><head><style>p{color:red}</style></head>\r\n" +
            "<body><p>Hello <b>there</b>.</p><p>Second line.</p>\r\n" +
            "<img src=\"https://tracker.example/pixel.gif?id=42\">\r\n" +
            "</body></html>\r\n"
        val out = MessageReader.display(raw(message))
        assertTrue(out.fromHtml)
        assertEquals("Hello there.\nSecond line.", out.text)
        // The whole point, asserted rather than assumed.
        assertFalse(out.text.contains("tracker.example"))
        assertFalse(out.text.contains("color:red"))
    }

    /**
     * The plain half of a two-part message wins, and nothing says it
     * came from markup, because it did not.
     */
    @Test
    fun `alternative reads as plain`() {
        val message = "Content-Type: multipart/alternative; boundary=\"b\"\r\n\r\n" +
            "--b\r\nContent-Type: text/plain\r\n\r\nthe readable one\r\n" +
            "--b\r\nContent-Type: text/html\r\n\r\n<p>the other one</p>\r\n--b--\r\n"
        val out = MessageReader.display(raw(message))
        assertTrue(out.text.contains("the readable one"))
        assertFalse(out.fromHtml)
    }

    /**
     * A message with nothing showable is empty text, not a crash and not
     * a failure — the screen says so in its own words.
     */
    @Test
    fun `an attachment only message has nothing to show`() {
        val message = "Content-Type: application/pdf\r\n\r\nJVBERi0="
        assertTrue(MessageReader.display(raw(message)).text.isEmpty())
    }
}
