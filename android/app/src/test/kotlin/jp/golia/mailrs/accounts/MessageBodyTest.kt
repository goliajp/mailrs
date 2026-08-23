package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Pulling the readable part out of a raw message. */
class MessageBodyTest {
    private fun raw(s: String) = s.toByteArray(Charsets.UTF_8)

    @Test
    fun `a plain message is its own body`() {
        val out = MessageBody.extract(raw("Subject: hi\r\n\r\nHello there.\r\n"))
        assertEquals("Hello there.\r\n", out.text)
        assertFalse(out.isHtml)
    }

    /**
     * No `Content-Type` at all is `text/plain; charset=us-ascii` by
     * RFC 2045 — and far more common than any declared type.
     */
    @Test
    fun `a message with no content type is still text`() {
        assertEquals("body", MessageBody.extract(raw("From: a\n\nbody")).text)
    }

    @Test
    fun `quoted printable is decoded`() {
        val message = "Content-Type: text/plain; charset=utf-8\r\n" +
            "Content-Transfer-Encoding: quoted-printable\r\n\r\n" +
            "caf=C3=A9 and a very long line that was wrapped right =\r\nhere"
        assertEquals(
            "café and a very long line that was wrapped right here",
            MessageBody.extract(raw(message)).text,
        )
    }

    @Test
    fun `base64 is decoded across lines`() {
        val body = java.util.Base64.getEncoder()
            .encodeToString("Hello, wrapped base64 body.".toByteArray())
        val wrapped = body.take(8) + "\r\n" + body.drop(8)
        val message = "Content-Transfer-Encoding: base64\r\n\r\n" + wrapped
        assertEquals("Hello, wrapped base64 body.", MessageBody.extract(raw(message)).text)
    }

    /**
     * The charset is inside the message, which is why this works on
     * bytes: decoding as UTF-8 on the way in would already have lost
     * these.
     */
    @Test
    fun `a declared charset is honoured`() {
        val head = "Content-Type: text/plain; charset=iso-8859-1\r\n\r\n".toByteArray()
        val message = head + byteArrayOf(0x63, 0x61, 0x66, 0xE9.toByte())
        assertEquals("café", MessageBody.extract(message).text)
    }

    /** The same message twice: show the one a person can read. */
    @Test
    fun `alternative prefers plain text`() {
        val message = "Content-Type: multipart/alternative; boundary=\"x\"\r\n\r\n" +
            "preamble nobody sees\r\n" +
            "--x\r\nContent-Type: text/plain\r\n\r\nthe plain one\r\n" +
            "--x\r\nContent-Type: text/html\r\n\r\n<p>the markup one</p>\r\n" +
            "--x--\r\nepilogue nobody sees\r\n"
        val out = MessageBody.extract(raw(message))
        assertTrue(out.text.contains("the plain one"))
        assertFalse(out.isHtml)
        assertFalse(out.text.contains("preamble"))
        assertFalse(out.text.contains("epilogue"))
    }

    /**
     * Markup when that is all there is — flagged, so the caller renders
     * it rather than showing somebody their own angle brackets.
     */
    @Test
    fun `alternative falls back to html`() {
        val message = "Content-Type: multipart/alternative; boundary=\"x\"\r\n\r\n" +
            "--x\r\nContent-Type: text/html\r\n\r\n<p>only markup</p>\r\n--x--\r\n"
        val out = MessageBody.extract(raw(message))
        assertTrue(out.isHtml)
        assertTrue(out.text.contains("only markup"))
    }

    /** A message with an attachment: the message is the message. */
    @Test
    fun `mixed skips the attachment`() {
        val message = "Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n" +
            "--b\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n" +
            "--b\r\nContent-Type: application/pdf\r\n" +
            "Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n"
        assertTrue(MessageBody.extract(raw(message)).text.contains("see attached"))
    }

    /**
     * A `mixed` whose first piece is an `alternative`, which is what
     * most mail with an attachment actually looks like.
     */
    @Test
    fun `a nested alternative is read through`() {
        val message = "Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n" +
            "--outer\r\nContent-Type: multipart/alternative; boundary=\"inner\"\r\n\r\n" +
            "--inner\r\nContent-Type: text/plain\r\n\r\nnested plain\r\n" +
            "--inner\r\nContent-Type: text/html\r\n\r\n<p>nested markup</p>\r\n" +
            "--inner--\r\n--outer--\r\n"
        val out = MessageBody.extract(raw(message))
        assertTrue(out.text.contains("nested plain"))
        assertFalse(out.isHtml)
    }

    /**
     * A boundary with a semicolon in it, quoted. Splitting the parameter
     * list on every semicolon loses the rest of the name and then
     * nothing matches.
     */
    @Test
    fun `a quoted boundary may span a semicolon`() {
        val message = "Content-Type: multipart/alternative; boundary=\"a;b\"\r\n\r\n" +
            "--a;b\r\nContent-Type: text/plain\r\n\r\nfound it\r\n--a;b--\r\n"
        assertTrue(MessageBody.extract(raw(message)).text.contains("found it"))
    }

    /** Nothing to show is not a crash. */
    @Test
    fun `broken input is empty rather than fatal`() {
        assertEquals(MessageBody.Display.EMPTY, MessageBody.extract(ByteArray(0)))
        assertTrue(MessageBody.extract(raw("Subject: only headers\r\n")).text.isEmpty())
        val noBoundary = "Content-Type: multipart/alternative\r\n\r\nsomething"
        assertTrue(MessageBody.extract(raw(noBoundary)).text.contains("something"))
    }

    /**
     * An attachment on its own is not text, and showing its bytes as
     * text is a screen of noise.
     */
    @Test
    fun `a non text part shows nothing`() {
        val message = "Content-Type: image/png\r\n\r\nPNG"
        assertEquals(MessageBody.Display.EMPTY, MessageBody.extract(raw(message)))
    }
}
