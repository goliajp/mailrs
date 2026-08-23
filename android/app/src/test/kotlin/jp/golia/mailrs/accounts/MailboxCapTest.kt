package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** How many rows one account may keep. */
class MailboxCapTest {
    private fun row(account: String, uid: Long, date: Long?) =
        MailboxRow(
            accountId = account, uid = uid, folder = "INBOX", seen = false,
            sender = "s", subject = "x", date = date, messageId = "m$uid",
        )

    /** Under the limit, nothing is touched at all. */
    @Test
    fun `a short list is returned as it was`() {
        val rows = (1L..10L).map { row("a", it, it) }
        assertEquals(rows, MailboxApply.capped(rows, limit = 100))
    }

    /**
     * What falls off is what somebody would have scrolled furthest to
     * see — the same order the list itself uses.
     */
    @Test
    fun `the oldest go first`() {
        val rows = (1L..10L).map { row("a", it, it) }
        val kept = MailboxApply.capped(rows, limit = 3)
        assertEquals(listOf(8L, 9L, 10L), kept.map { it.uid })
    }

    /**
     * **Per account, not overall.** One noisy mailbox would otherwise
     * evict a quiet one entirely, and the quiet one is where the mail
     * somebody is waiting for tends to be.
     */
    @Test
    fun `a noisy account cannot evict a quiet one`() {
        val noisy = (1L..100L).map { row("noisy", it, 1_000 + it) }
        val quiet = listOf(row("quiet", 1L, 1L))
        val kept = MailboxApply.capped(noisy + quiet, limit = 10)
        assertEquals(10, kept.count { it.accountId == "noisy" })
        assertEquals(1, kept.count { it.accountId == "quiet" })
    }

    /**
     * The stored order survives: the list sorts rows itself, and
     * reshuffling storage on every pass makes a diff unreadable.
     */
    @Test
    fun `the held order is not rearranged`() {
        val rows = listOf(row("a", 3L, 3L), row("a", 1L, 1L), row("a", 2L, 2L))
        val kept = MailboxApply.capped(rows, limit = 2)
        assertEquals(listOf(3L, 2L), kept.map { it.uid })
    }

    /** A row with no date is still a row, and it is not preferred away. */
    @Test
    fun `rows without a date are kept when there is room`() {
        val rows = listOf(row("a", 1L, null), row("a", 2L, 5L))
        assertTrue(MailboxApply.capped(rows, limit = 5).size == 2)
    }
}
