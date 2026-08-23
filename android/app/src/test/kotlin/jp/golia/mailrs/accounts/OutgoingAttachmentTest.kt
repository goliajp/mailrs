package jp.golia.mailrs.accounts

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.TimeZone

/**
 * Sending a file, checked by reading it back.
 *
 * The builder and the parser are two halves of this app that never
 * meet in production — one writes what leaves, the other reads what
 * arrives — so pointing one at the other is the only check that the
 * message this app sends is a message this app could receive.
 */
class OutgoingAttachmentTest {
    private val whenSeconds = 1_756_000_000L
    private val utc: TimeZone = TimeZone.getTimeZone("UTC")

    private fun built(vararg attachments: OutgoingMessage.Attachment): String {
        val draft = OutgoingMessage.Draft(
            from = "me@example.com",
            to = listOf("you@example.com"),
            subject = "Here it is",
            body = "See attached.",
            attachments = attachments.toList(),
        )
        return OutgoingMessage.text(draft, "x@example.com", whenSeconds, utc)
    }

    /** No attachments is the plain message it always was. */
    @Test
    fun `a message with no attachment is not multipart`() {
        val message = built()
        assertTrue(message, message.contains("Content-Type: text/plain; charset=utf-8"))
        assertFalse(message, message.contains("multipart"))
    }

    /**
     * **Read back with this app's own parser.** The bytes that come
     * out must be the bytes that went in — base64, line wrapping,
     * boundaries and all.
     */
    @Test
    fun `an attachment survives being written and read`() {
        val payload = ByteArray(5000) { (it % 251).toByte() }
        val message = built(
            OutgoingMessage.Attachment("report 2025.pdf", "application/pdf", payload),
        )
        val raw = message.toByteArray(Charsets.ISO_8859_1)

        val found = MessageAttachments.of(raw)
        assertEquals(1, found.size)
        assertEquals("report 2025.pdf", found[0].filename)
        assertEquals("application/pdf", found[0].mimeType)
        assertArrayEquals(payload, found[0].bytes)
    }

    /**
     * **The text part comes first.** Every reader shows the first text
     * part it finds, and a message whose first part is a PDF opens as
     * a PDF with the words underneath it.
     */
    @Test
    fun `the words are still the body`() {
        val message = built(
            OutgoingMessage.Attachment("a.pdf", "application/pdf", byteArrayOf(1, 2, 3)),
        )
        val body = MessageBody.extract(message.toByteArray(Charsets.ISO_8859_1))
        assertEquals("See attached.\r\n", body.text)
        assertFalse(body.isHtml)
    }

    /** Several files come back as several files, in order. */
    @Test
    fun `two attachments are two attachments`() {
        val message = built(
            OutgoingMessage.Attachment("one.txt", "text/plain", "first".toByteArray()),
            OutgoingMessage.Attachment("two.bin", "application/octet-stream", byteArrayOf(0, 1)),
        )
        val found = MessageAttachments.of(message.toByteArray(Charsets.ISO_8859_1))
        assertEquals(listOf("one.txt", "two.bin"), found.map { it.filename })
        assertEquals("first", String(found[0].bytes))
    }

    /**
     * **A filename cannot break the header it sits in.** A quote ends
     * the quoted string early and a newline ends the header — which is
     * how a filename becomes an injected header.
     */
    @Test
    fun `a filename cannot inject a header`() {
        val nasty = "in\"voice\r\nBcc: someone@else.example\r\n.pdf"
        val message = built(
            OutgoingMessage.Attachment(nasty, "application/pdf", byteArrayOf(9)),
        )
        // **The property is that no line is a header it should not
        // be** — not that the letters are absent. The name keeps
        // `Bcc:` as text once the newlines are stripped, which is
        // fine: a filename is allowed to contain a colon. What must
        // not exist is a *line* that starts one.
        val injected = message.split("\r\n").any { it.startsWith("Bcc:", ignoreCase = true) }
        assertFalse("a header was injected through a filename", injected)
        // And the header it belongs to is still one line.
        val disposition = message.split("\r\n").filter { it.startsWith("Content-Disposition:") }
        assertEquals(1, disposition.size)
        // And it still round-trips as one attachment.
        val found = MessageAttachments.of(message.toByteArray(Charsets.ISO_8859_1))
        assertEquals(1, found.size)
    }

    /**
     * The boundary is derived from the message id, which is already
     * unique — a boundary that turns up inside a part cuts the message
     * in half at that point.
     */
    @Test
    fun `the boundary does not appear inside the message`() {
        val message = built(
            OutgoingMessage.Attachment("a.bin", "application/octet-stream", ByteArray(3000)),
        )
        val boundary = Regex("boundary=\"([^\"]+)\"").find(message)!!.groupValues[1]
        // Three: the two parts and the close.
        assertEquals(3, Regex(Regex.escape("--$boundary")).findAll(message).count())
    }

    /** Base64 is wrapped, as RFC 2045 asks, and still decodes. */
    @Test
    fun `base64 is wrapped at seventy six`() {
        val message = built(
            OutgoingMessage.Attachment("a.bin", "application/octet-stream", ByteArray(1000)),
        )
        val longest = message.split("\r\n").maxOf { it.length }
        assertTrue(longest.toString(), longest <= 78)
    }
}
