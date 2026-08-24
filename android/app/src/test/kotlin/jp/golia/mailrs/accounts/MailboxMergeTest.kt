package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Putting several mailboxes into one list. */
class MailboxMergeTest {
    private fun row(
        account: String,
        uid: Long,
        date: Long?,
        seen: Boolean = false,
        folder: String = "INBOX",
        subject: String = "s",
    ) = MailboxRow(account, uid, folder, seen, "a@x.jp", subject, date, "<$account-$uid>")

    // A uid is unique within one folder of one account and nowhere
    // else: two accounts both have a message 1, and a list keyed on uid
    // alone shows one of them twice and the other never.
    @Test
    fun `two accounts can both have message one`() {
        assertNotEquals(row("acc_1", 1, 100).id, row("acc_2", 1, 100).id)
        // And the same message in two folders of one account, which
        // Gmail does with labels.
        assertNotEquals(
            row("acc_1", 1, 1, folder = "INBOX").id,
            row("acc_1", 1, 1, folder = "Archive").id,
        )
    }

    @Test
    fun `the newest is first`() {
        val out = MailboxMerge.newestFirst(
            listOf(row("a", 1, 100), row("b", 2, 300), row("c", 3, 200)),
        )
        assertEquals(listOf(2L, 3L, 1L), out.map { it.uid })
    }

    // A mailing list fans one message out to a hundred people in the
    // same second. Without a tie-break the order changes between two
    // calls with the same input.
    @Test
    fun `a tie is broken by something stable`() {
        val rows = listOf(row("b", 2, 100), row("a", 1, 100), row("c", 3, 100))
        assertEquals(
            MailboxMerge.newestFirst(rows).map { it.id },
            MailboxMerge.newestFirst(rows.reversed()).map { it.id },
        )
    }

    // The one thing this client knows nothing about must not take the
    // position that says "newest".
    @Test
    fun `a row with no date sorts last`() {
        val out = MailboxMerge.newestFirst(listOf(row("a", 1, null), row("b", 2, 100)))
        assertEquals(listOf(2L, 1L), out.map { it.uid })
    }

    // `null` is no filter; **empty** is a filter nothing satisfies.
    @Test
    fun `no filter and an empty filter are different questions`() {
        val rows = listOf(row("a", 1, 1), row("b", 2, 2))
        assertEquals(2, MailboxMerge.onlyAccounts(rows, null).size)
        assertTrue(MailboxMerge.onlyAccounts(rows, emptySet()).isEmpty())
        assertEquals(listOf(1L), MailboxMerge.onlyAccounts(rows, setOf("a")).map { it.uid })
    }

    @Test
    fun `the unread count counts unread`() {
        val rows = listOf(
            row("a", 1, 1, seen = false),
            row("a", 2, 2, seen = true),
            row("b", 3, 3, seen = false),
        )
        assertEquals(2, MailboxMerge.unreadCount(rows))
    }

    // Plenty of real mail has no subject, and an empty line in a list
    // reads as a rendering fault.
    @Test
    fun `a message with no subject still has a line`() {
        assertEquals("(no subject)", row("a", 1, 1, subject = "").displaySubject)
        assertEquals("(no subject)", row("a", 1, 1, subject = "   ").displaySubject)
        assertEquals("real", row("a", 1, 1, subject = "real").displaySubject)
    }

    // The list hoists the sort out of the search so typing does not
    // re-sort every row. That is only safe if searching a sorted list
    // and sorting a searched one give the same answer — which they do
    // because the search filters and never reorders, and this is the
    // assertion that says so.
    @Test
    fun `searching after sorting is the same as sorting after searching`() {
        val rows = listOf(
            row("a", 1, date = 300, subject = "Lunch"),
            row("a", 2, date = 100, subject = "Lunch tomorrow"),
            row("b", 3, date = 200, subject = "Dinner"),
        )
        val sortedThenSearched =
            MailboxSearch.matches(MailboxMerge.newestFirst(rows), "lunch")
        val searchedThenSorted =
            MailboxMerge.newestFirst(MailboxSearch.matches(rows, "lunch"))
        assertEquals(searchedThenSorted, sortedThenSearched)
    }
}

/** Folding a pass's worth of rows into what is already held. */
class MailboxApplyTest {
    private fun row(
        account: String,
        uid: Long,
        date: Long?,
        seen: Boolean = false,
        folder: String = "INBOX",
    ) = MailboxRow(account, uid, folder, seen, "a@x.jp", "s", date, "<$account-$uid>")

    // The five assertions that used to sit here — a message read twice
    // is one row, the server's flags win, new messages are kept, a
    // renumbered folder is replaced rather than merged, and removing an
    // account takes its mail — moved to MailboxDatabaseTest when the
    // rows moved into SQLite. They are properties of the store, and the
    // store is now the table; asserting them against a list that no
    // production code builds any more would have been a suite that
    // stays green while the thing it names breaks.
}