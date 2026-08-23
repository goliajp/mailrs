package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Finding a message in what has already been fetched. */
class MailboxSearchTest {
    private fun row(sender: String, subject: String, folder: String = "INBOX") =
        MailboxRow(
            accountId = "a", uid = sender.hashCode().toLong(), folder = folder,
            seen = false, sender = sender, subject = subject, date = null, messageId = "m",
        )

    private val rows = listOf(
        row("Ada <ada@example.com>", "Lunch on Thursday"),
        row("Bob <bob@example.com>", "Invoice 4471"),
        row("Ada <ada@example.com>", "Re: Invoice 4471"),
        row("会議事務局 <mtg@example.jp>", "会議のお知らせ"),
    )

    /** Nothing typed is not a filter. */
    @Test
    fun `an empty query matches everything`() {
        assertEquals(rows.size, MailboxSearch.matches(rows, "").size)
        assertEquals(rows.size, MailboxSearch.matches(rows, "   ").size)
    }

    /** Case is not something anybody types deliberately. */
    @Test
    fun `matching ignores case`() {
        assertEquals(2, MailboxSearch.matches(rows, "ADA").size)
        assertEquals(2, MailboxSearch.matches(rows, "invoice").size)
    }

    /**
     * **Every** word, not any: somebody typing two words is narrowing,
     * and a search that widens with each word gets further from what
     * they want the more they say.
     */
    @Test
    fun `every word must match`() {
        assertEquals(1, MailboxSearch.matches(rows, "ada invoice").size)
        assertTrue(MailboxSearch.matches(rows, "ada lunch invoice").isEmpty())
    }

    /**
     * The words may match different fields — "ada lunch" is a message
     * from Ada about lunch, which is how people search and not how a
     * naive substring match behaves.
     */
    @Test
    fun `words may match different fields`() {
        val found = MailboxSearch.matches(rows, "ada lunch")
        assertEquals(1, found.size)
        assertEquals("Lunch on Thursday", found[0].subject)
    }

    /** No spaces between words is not a reason to find nothing. */
    @Test
    fun `cjk matches as a substring`() {
        assertEquals(1, MailboxSearch.matches(rows, "会議").size)
        assertEquals(1, MailboxSearch.matches(rows, "お知らせ").size)
    }

    /** The folder is searchable too — it is on the row and on screen. */
    @Test
    fun `the folder name is searchable`() {
        val filed = rows + row("Carol <c@example.com>", "Receipt", folder = "Archive")
        assertEquals(1, MailboxSearch.matches(filed, "archive").size)
    }
}
