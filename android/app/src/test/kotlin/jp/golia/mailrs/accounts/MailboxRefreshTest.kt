package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** What a pass learned about messages this device already had. */
class MailboxRefreshTest {
    private fun row(uid: Long, seen: Boolean, folder: String = "INBOX", account: String = "a") =
        MailboxRow(
            accountId = account, uid = uid, folder = folder, seen = seen,
            sender = "s", subject = "x", date = null, messageId = "m$uid",
        )

    /** A message read on a laptop stops being bold here. */
    @Test
    fun `a flag changed elsewhere is picked up`() {
        val held = listOf(row(1, false), row(2, false))
        val after = MailboxRefresh.apply(
            held, "a", "INBOX", setOf(1L, 2L), mapOf(1L to true, 2L to false),
        )
        assertEquals(true, after[0].seen)
        assertEquals(false, after[1].seen)
    }

    /**
     * **A uid asked about that did not come back is gone.** Without
     * this, a message deleted on another device stays in the list
     * forever.
     */
    @Test
    fun `a message deleted elsewhere is removed`() {
        val held = listOf(row(1, false), row(2, false), row(3, false))
        val after = MailboxRefresh.apply(
            held, "a", "INBOX", setOf(1L, 2L, 3L), mapOf(1L to false, 3L to false),
        )
        assertEquals(listOf(1L, 3L), after.map { it.uid })
    }

    /**
     * **Only rows that were asked about may be removed.** A partial or
     * interrupted fetch would otherwise empty the list — the answer is
     * silent about everything the question did not name.
     */
    @Test
    fun `a row that was not asked about survives an empty answer`() {
        val held = listOf(row(1, false), row(2, false))
        val after = MailboxRefresh.apply(held, "a", "INBOX", setOf(1L), emptyMap())
        assertEquals(listOf(2L), after.map { it.uid })
    }

    /** And an answer about one folder says nothing about another. */
    @Test
    fun `another folder is untouched`() {
        val held = listOf(row(1, false), row(1, false, folder = "Sent"))
        val after = MailboxRefresh.apply(held, "a", "INBOX", setOf(1L), emptyMap())
        assertEquals(1, after.size)
        assertEquals("Sent", after[0].folder)
    }

    /** Nor about another account, whose uids look exactly the same. */
    @Test
    fun `another account is untouched`() {
        val held = listOf(row(1, false), row(1, false, account = "b"))
        val after = MailboxRefresh.apply(held, "a", "INBOX", setOf(1L), emptyMap())
        assertEquals(1, after.size)
        assertEquals("b", after[0].accountId)
    }

    /** Nothing asked, nothing changed. */
    @Test
    fun `asking about nothing changes nothing`() {
        val held = listOf(row(1, true), row(2, false))
        assertEquals(held, MailboxRefresh.apply(held, "a", "INBOX", emptySet(), emptyMap()))
        assertTrue(MailboxRefresh.apply(emptyList(), "a", "INBOX", setOf(1L), emptyMap()).isEmpty())
    }
}
