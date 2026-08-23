package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Test

/** The few headers a list row needs. */
class MessageHeadersTest {
    private val message = listOf(
        "From: Alice Smith <alice@example.com>",
        "To: bob@golia.jp",
        "Subject: Quarterly report",
        "Message-ID: <m1@example.com>",
        "Date: Tue, 5 Aug 2026 09:00:00 +0900",
        "",
        "Subject: this is body text, not a header",
    ).joinToString("\r\n")

    @Test
    fun `the row's fields are read`() {
        val p = MessageHeaders.parse(message)
        assertEquals("Alice Smith <alice@example.com>", p.from)
        assertEquals("Quarterly report", p.subject)
        assertEquals("<m1@example.com>", p.messageId)
    }

    // A body may contain anything, including lines that look exactly
    // like headers. A parser that keeps going reads them.
    @Test
    fun `the body is not read as headers`() {
        assertEquals("Quarterly report", MessageHeaders.parse(message).subject)
    }

    // **Folding is the trap.** A long Subject continues on the next
    // line, and a parser that reads lines gets half of it.
    @Test
    fun `a folded subject is whole`() {
        val raw = "Subject: Quarterly report and\r\n the follow-up notes\r\n\r\nbody"
        assertEquals(
            "Quarterly report and the follow-up notes",
            MessageHeaders.parse(raw).subject,
        )
    }

    // And the continuation must not become a header of its own.
    @Test
    fun `a continuation is not its own header`() {
        val raw = "Subject: one\r\n Date: not-a-date\r\n" +
            "Date: Tue, 5 Aug 2026 09:00:00 +0900\r\n\r\n"
        val p = MessageHeaders.parse(raw)
        assertEquals("one Date: not-a-date", p.subject)
        assertEquals("Tue, 5 Aug 2026 09:00:00 +0900", p.date)
    }

    // A message with two Subjects is malformed, and picking the last
    // lets somebody append one.
    @Test
    fun `a repeated header keeps the first`() {
        assertEquals("real", MessageHeaders.parse("Subject: real\r\nSubject: injected\r\n\r\n").subject)
    }

    @Test
    fun `a display name is preferred to an address`() {
        assertEquals("Alice Smith", MessageHeaders.senderName("Alice Smith <alice@example.com>"))
        assertEquals("alice@example.com", MessageHeaders.senderName("alice@example.com"))
        assertEquals("alice@example.com", MessageHeaders.senderName("<alice@example.com>"))
    }

    // A name with a comma is quoted, and the quotes are syntax.
    @Test
    fun `quotes come off a name`() {
        assertEquals("Smith, Alice", MessageHeaders.senderName("\"Smith, Alice\" <a@x.com>"))
    }
}

/**
 * The subject and the sender arrive decoded.
 *
 * Every reader of a subject wants the text; one that forgets to decode
 * shows `=?UTF-8?B?` to somebody. Doing it at the door means no reader
 * can forget.
 */
class DecodedHeadersTest {
    @Test
    fun `a japanese subject is readable`() {
        val raw = "Subject: =?UTF-8?B?5Lya6K2w44Gu5Lu2?=\r\nFrom: a@x.jp\r\n\r\n"
        assertEquals("会議の件", MessageHeaders.parse(raw).subject)
    }

    @Test
    fun `an encoded display name is readable`() {
        val raw = "From: =?UTF-8?B?55Sw5Lit?= <tanaka@x.jp>\r\n\r\n"
        val p = MessageHeaders.parse(raw)
        assertEquals("田中", MessageHeaders.senderName(p.from))
    }

    // A folded, encoded subject is both traps at once: unfold first,
    // then decode, or the two halves decode separately and the gap
    // between them survives as a space.
    @Test
    fun `a folded encoded subject is whole and readable`() {
        val raw = "Subject: =?UTF-8?B?5Lya6K2w?=\r\n =?UTF-8?B?44Gu5Lu2?=\r\n\r\n"
        assertEquals("会議の件", MessageHeaders.parse(raw).subject)
    }
}
