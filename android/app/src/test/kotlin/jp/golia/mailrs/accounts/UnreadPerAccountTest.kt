package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Unread per account, for the filter to say which is worth looking at. */
class UnreadPerAccountTest {
    private fun row(account: String, seen: Boolean, uid: Long) =
        MailboxRow(
            accountId = account, uid = uid, folder = "INBOX", seen = seen,
            sender = "s", subject = "x", date = null, messageId = "m$uid",
        )

    @Test
    fun `each account is counted apart`() {
        val rows = listOf(
            row("a", false, 1), row("a", false, 2), row("a", true, 3),
            row("b", false, 4),
        )
        assertEquals(mapOf("a" to 2, "b" to 1), MailboxMerge.unreadPerAccount(rows))
    }

    /**
     * **Accounts with none are absent, not zero.** A badge reading `0`
     * says nothing while taking the space of one that would, and every
     * mail client hides it.
     */
    @Test
    fun `an account with nothing unread is absent`() {
        val rows = listOf(row("a", true, 1), row("b", false, 2))
        val counts = MailboxMerge.unreadPerAccount(rows)
        assertFalse("an account with nothing unread got a badge", counts.containsKey("a"))
        assertEquals(1, counts["b"])
    }

    /** Nothing at all is an empty map rather than a crash. */
    @Test
    fun `no rows is no counts`() {
        assertTrue(MailboxMerge.unreadPerAccount(emptyList()).isEmpty())
    }
}
