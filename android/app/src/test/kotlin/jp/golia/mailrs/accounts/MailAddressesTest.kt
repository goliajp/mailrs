package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Test

/** Reading an address list out of a header. */
class MailAddressesTest {
    /**
     * The whole of the difficulty is one character: a display name may
     * contain a comma, which is why it is quoted. Splitting on every
     * comma makes two recipients out of one, and one of them is
     * nonsense.
     */
    @Test
    fun `a comma inside a quoted name is not a separator`() {
        assertEquals(
            listOf("\"Lovelace, Ada\" <ada@example.com>", "bob@example.com"),
            MailAddresses.split("\"Lovelace, Ada\" <ada@example.com>, bob@example.com"),
        )
    }

    /** Nor is one inside angle brackets, where an obsolete route lives. */
    @Test
    fun `a comma inside angle brackets is not a separator`() {
        assertEquals(
            listOf("<@a.example,@b.example:c@d.example>", "e@f.example"),
            MailAddresses.split("<@a.example,@b.example:c@d.example>, e@f.example"),
        )
    }

    /** The ordinary case stays ordinary. */
    @Test
    fun `plain lists split on commas`() {
        assertEquals(
            listOf("a@b.com", "c@d.com"),
            MailAddresses.split("a@b.com,  c@d.com"),
        )
        assertEquals(emptyList<String>(), MailAddresses.split(""))
        assertEquals(emptyList<String>(), MailAddresses.split("  ,  "))
    }

    /**
     * For comparing, never for showing: `Ada <a@b>` and `a@b` are the
     * same person, and a reply-all that does not know it copies
     * somebody to their own message.
     */
    @Test
    fun `the bare address ignores the display name and the case`() {
        assertEquals("ada@example.com", MailAddresses.bare("Ada <Ada@Example.COM>"))
        assertEquals("ada@example.com", MailAddresses.bare("  ada@example.com "))
        assertEquals("ada@example.com", MailAddresses.bare("\"A, B\" <ada@example.com>"))
    }

    /**
     * A reply-all that copies its own author is the thing everybody
     * notices, and the thing nobody can undo once it is sent.
     */
    @Test
    fun `reply all never copies the sender or the primary recipient`() {
        val copies = MailAddresses.replyAll(
            to = "Me <me@example.com>, Ada <ada@example.com>",
            cc = "bob@example.com",
            primary = "Ada <ada@example.com>",
            mine = "me@example.com",
        )
        assertEquals(listOf("bob@example.com"), copies)
    }

    /** And nobody appears twice, however they were written. */
    @Test
    fun `somebody on both to and cc is copied once`() {
        val copies = MailAddresses.replyAll(
            to = "Bob <BOB@example.com>",
            cc = "bob@example.com, carol@example.com",
            primary = "ada@example.com",
            mine = "me@example.com",
        )
        assertEquals(listOf("Bob <BOB@example.com>", "carol@example.com"), copies)
    }

    /** The order people were written in is the order they stay in. */
    @Test
    fun `the written order is kept`() {
        val copies = MailAddresses.replyAll(
            to = "z@example.com, a@example.com",
            cc = "m@example.com",
            primary = "x@example.com",
            mine = "me@example.com",
        )
        assertEquals(listOf("z@example.com", "a@example.com", "m@example.com"), copies)
    }
}
