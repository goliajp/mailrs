package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** What is attached to a message. */
class MessageAttachmentsTest {
    private fun raw(s: String) = s.toByteArray(Charsets.UTF_8)

    /** A plain message has nothing attached, and says so with a list. */
    @Test
    fun `a message with nothing attached has nothing`() {
        assertTrue(MessageAttachments.of(raw("Subject: hi\r\n\r\nbody")).isEmpty())
    }

    /**
     * The part a reader sees is not attached, and a PDF nobody can
     * render still has to be listed — which is why this is a different
     * question from what to show.
     */
    @Test
    fun `the body is not attached and the pdf is`() {
        val message = "Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n" +
            "--b\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n" +
            "--b\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n" +
            "Content-Transfer-Encoding: base64\r\n\r\nSGVsbG8=\r\n--b--\r\n"
        val found = MessageAttachments.of(raw(message))
        assertEquals(1, found.size)
        assertEquals("report.pdf", found[0].filename)
        assertEquals("application/pdf", found[0].mimeType)
        assertEquals("Hello", String(found[0].bytes, Charsets.UTF_8))
    }

    /**
     * A text part **with a filename** is attached — that is how a
     * `.txt` or a `.csv` arrives, and treating it as the body shows
     * the reader a spreadsheet instead of the message.
     */
    @Test
    fun `a named text part is an attachment`() {
        val message = "Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n" +
            "--b\r\nContent-Type: text/plain\r\n\r\nthe message\r\n" +
            "--b\r\nContent-Type: text/csv\r\n" +
            "Content-Disposition: attachment; filename=\"rows.csv\"\r\n\r\na,b\r\n--b--\r\n"
        val found = MessageAttachments.of(raw(message))
        assertEquals(1, found.size)
        assertEquals("rows.csv", found[0].filename)
    }

    /**
     * An inline image is listed anyway — a reader shown text has no
     * other way to reach it — but marked, so the list can say which is
     * which.
     */
    @Test
    fun `an inline image is listed and marked`() {
        val message = "Content-Type: multipart/related; boundary=\"b\"\r\n\r\n" +
            "--b\r\nContent-Type: text/html\r\n\r\n<p>hi</p>\r\n" +
            "--b\r\nContent-Type: image/png\r\n" +
            "Content-Disposition: inline; filename=\"sig.png\"\r\n" +
            "Content-ID: <sig>\r\n\r\nPNG\r\n--b--\r\n"
        val found = MessageAttachments.of(raw(message))
        assertEquals(1, found.size)
        assertTrue(found[0].inline)
    }

    /**
     * RFC 2231 is how a Japanese filename survives a header that must
     * be ASCII. A client that does not decode it shows the person
     * `%E6%97%A5%E6%9C%AC.pdf`.
     */
    @Test
    fun `an encoded filename is decoded`() {
        val header = "attachment; filename*=utf-8''%E6%97%A5%E6%9C%AC.pdf"
        assertEquals("日本.pdf", MessageAttachments.rfc2231(header, "filename"))
    }

    /** A long name is split across numbered continuations. */
    @Test
    fun `a filename split across continuations is rejoined`() {
        val header = "attachment; filename*0*=utf-8''%E6%97%A5; filename*1*=%E6%9C%AC.pdf"
        assertEquals("日本.pdf", MessageAttachments.rfc2231(header, "filename"))
    }

    /** And the ordinary quoted form still works. */
    @Test
    fun `a plain quoted filename works`() {
        assertEquals(
            "report 2025.pdf",
            MessageAttachments.rfc2231("attachment; filename=\"report 2025.pdf\"", "filename"),
        )
    }

    /**
     * `Content-Type: ...; name=` is the older place and still arrives,
     * so both are looked in.
     */
    @Test
    fun `the older name parameter is found too`() {
        val header = "Content-Type: application/zip; name=\"archive.zip\"\r\n"
        assertEquals("archive.zip", MessageAttachments.filename(header))
    }

    /**
     * Something to call a nameless part — not "attachment", because a
     * list of four things all called that is a list nobody can pick
     * from.
     */
    @Test
    fun `a nameless part is named after its type`() {
        val message = "Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n" +
            "--b\r\nContent-Type: text/plain\r\n\r\nmsg\r\n" +
            "--b\r\nContent-Type: image/jpeg\r\n\r\nJPEG\r\n--b--\r\n"
        val found = MessageAttachments.of(raw(message))
        assertEquals(1, found.size)
        assertEquals("image.jpg", found[0].filename)
        assertFalse(found[0].inline)
    }

    /** Two decodings of the same message are the same attachment. */
    @Test
    fun `attachments compare by content and not by array identity`() {
        val message = "Content-Type: application/pdf; name=\"a.pdf\"\r\n\r\nbytes"
        assertEquals(MessageAttachments.of(raw(message)), MessageAttachments.of(raw(message)))
    }

    /**
     * Two files may share a name, and a list that collapses them shows
     * one of them twice and the other never.
     */
    @Test
    fun `two files with one name are two attachments`() {
        val message = "Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n" +
            "--b\r\nContent-Type: application/pdf; name=\"a.pdf\"\r\n\r\none\r\n" +
            "--b\r\nContent-Type: application/pdf; name=\"a.pdf\"\r\n\r\ntwo bytes\r\n--b--\r\n"
        val found = MessageAttachments.of(raw(message))
        assertEquals(2, found.size)
        assertFalse(found[0] == found[1])
    }
}
