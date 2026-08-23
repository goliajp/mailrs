package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.TimeZone

/** Building the message that goes on the wire. */
class OutgoingMessageTest {
    private val whenSeconds = 1_756_000_000L // 2025-08-24 01:46:40Z
    private val tokyo = TimeZone.getTimeZone("Asia/Tokyo")

    private fun draft() = OutgoingMessage.Draft(
        from = "me@example.com",
        to = listOf("you@example.com"),
    )

    /**
     * A numeric offset, never a zone name: `JST` and the rest are
     * obsolete and ambiguous, and a client entitled to read them as UTC
     * moves the message by hours.
     */
    @Test
    fun `the date carries a numeric offset`() {
        val header = MailDate.rfc5322(whenSeconds, tokyo)
        assertEquals("Sun, 24 Aug 2025 10:46:40 +0900", header)
        assertFalse(header.contains("JST"))
    }

    /** And it must survive its own parser, which is the only check that matters. */
    @Test
    fun `the date round trips`() {
        for (zone in listOf("Asia/Tokyo", "UTC", "America/Los_Angeles", "Asia/Kolkata")) {
            val header = MailDate.rfc5322(whenSeconds, TimeZone.getTimeZone(zone))
            assertEquals(zone + ": " + header, whenSeconds, MailDate.epochSeconds(header))
        }
    }

    /**
     * A plain subject is left alone — encoding it makes the raw message
     * unreadable and gains nothing.
     */
    @Test
    fun `an ascii subject is not encoded`() {
        assertEquals("Lunch on Thursday", EncodedWord.encode("Lunch on Thursday"))
    }

    /** One that needs encoding survives its own decoder. */
    @Test
    fun `an encoded subject round trips`() {
        for (subject in listOf("会議のお知らせ", "café ☕", "Grüße aus Köln")) {
            val encoded = EncodedWord.encode(subject)
            assertTrue(encoded, encoded.startsWith("=?utf-8?B?"))
            assertEquals(encoded, subject, EncodedWord.decode(encoded))
        }
    }

    /**
     * A long one is folded, and each piece must still decode — the trap
     * is splitting UTF-8 through a character, which decodes to a
     * replacement character on every client.
     */
    @Test
    fun `a long encoded subject folds without breaking a character`() {
        val subject = "日本語の件名です。".repeat(12)
        val encoded = EncodedWord.encode(subject)
        assertTrue("a long subject was never folded", encoded.contains("\r\n "))
        for (line in encoded.split("\r\n")) assertTrue(line, line.trim().length <= 75)
        assertEquals(subject, EncodedWord.decode(encoded))
    }

    /**
     * An emoji is a surrogate pair in Kotlin, so splitting by `Char`
     * rather than by code point breaks it in half one level below where
     * UTF-8 would.
     */
    @Test
    fun `a run of emoji never splits a surrogate pair`() {
        val subject = "🎌".repeat(40)
        assertEquals(subject, EncodedWord.decode(EncodedWord.encode(subject)))
    }

    /** A name with a comma is two recipients to a parser that reads the comma. */
    @Test
    fun `a name with specials is quoted`() {
        assertEquals(
            "\"Lovelace, Ada\" <a@b.com>",
            OutgoingMessage.address("Lovelace, Ada", "a@b.com"),
        )
        assertEquals("Ada Lovelace <a@b.com>", OutgoingMessage.address("Ada Lovelace", "a@b.com"))
        assertEquals("a@b.com", OutgoingMessage.address("", "a@b.com"))
    }

    /**
     * An encoded word is already safe, and quoting one stops it being
     * decoded at all.
     */
    @Test
    fun `an encoded name is not quoted`() {
        val out = OutgoingMessage.address("山田 太郎", "a@b.com")
        assertTrue(out, out.startsWith("=?utf-8?B?"))
        assertFalse(out, out.startsWith("\""))
    }

    /**
     * Bcc lives in the envelope and nowhere else. Writing the header is
     * how a blind copy stops being blind.
     */
    @Test
    fun `bcc is in the envelope and not in the headers`() {
        val d = draft().copy(cc = listOf("cc@example.com"))
        assertEquals(
            listOf("you@example.com", "cc@example.com", "secret@example.com"),
            OutgoingMessage.envelope(d, listOf("secret@example.com")),
        )
        val message = OutgoingMessage.text(d, "x@example.com", whenSeconds, tokyo)
        assertTrue(message.contains("Cc: cc@example.com"))
        assertFalse(message.lowercase().contains("bcc"))
        assertFalse(message.contains("secret@example.com"))
    }

    /** The same person on To and Cc is one delivery, not two. */
    @Test
    fun `a duplicate recipient is delivered once`() {
        val d = draft().copy(cc = listOf("YOU@example.com", " "))
        assertEquals(listOf("you@example.com"), OutgoingMessage.envelope(d))
    }

    /**
     * A reply that carries neither header starts a new conversation in
     * every client that reads it.
     */
    @Test
    fun `a reply carries its threading`() {
        val d = draft().copy(
            inReplyTo = "<parent@example.com>",
            references = listOf("<grandparent@example.com>"),
        )
        val message = OutgoingMessage.text(d, "x@example.com", whenSeconds, tokyo)
        assertTrue(message.contains("In-Reply-To: <parent@example.com>"))
        assertTrue(
            message.contains("References: <grandparent@example.com> <parent@example.com>"),
        )
    }

    /** A body with bare newlines arrives as one long line. */
    @Test
    fun `every line ends crlf`() {
        val d = draft().copy(body = "one\ntwo\r\nthree\rfour")
        val message = OutgoingMessage.text(d, "x@example.com", whenSeconds, tokyo)
        assertTrue(message.endsWith("one\r\ntwo\r\nthree\r\nfour\r\n"))
        assertFalse(message.replace("\r\n", "").contains("\n"))
    }

    /**
     * The header block ends with a blank line, and the body starts after
     * it. Off by one here and the first line of the body is read as a
     * header.
     */
    @Test
    fun `the header block is separated from the body`() {
        val d = draft().copy(subject = "Hi", body = "Body starts here.")
        val message = OutgoingMessage.text(d, "x@example.com", whenSeconds, tokyo)
        assertEquals(
            "Body starts here.\r\n",
            MessageBody.extract(message.toByteArray(Charsets.UTF_8)).text,
        )
        assertEquals("Hi", MessageHeaders.parse(message).subject)
        assertEquals("<x@example.com>", MessageHeaders.parse(message).messageId)
    }

    /**
     * A message this builder makes must be readable by the reader on the
     * other side of this same app.
     */
    @Test
    fun `what is built is what is read`() {
        val d = draft().copy(
            fromName = "山田 太郎",
            subject = "会議のお知らせ",
            body = "本文です。\n二行目。",
        )
        val message = OutgoingMessage.text(d, "x@example.com", whenSeconds, tokyo)
        val headers = MessageHeaders.parse(message)
        assertEquals("会議のお知らせ", headers.subject)
        assertTrue(headers.from, headers.from.contains("山田 太郎"))
        assertEquals(whenSeconds, MailDate.epochSeconds(headers.date))
        assertEquals(
            "本文です。\r\n二行目。\r\n",
            MessageReader.display(message.toByteArray(Charsets.UTF_8)).text,
        )
    }
}
