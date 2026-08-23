package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** How old what is on screen is, across every account. */
class OldestSyncTest {
    /**
     * **The oldest, not the newest.** With two accounts synced a
     * minute ago and one failing since yesterday, "updated just now"
     * is a lie about the third — and telling "no new mail" apart from
     * "we have not managed to check" is the whole reason the line
     * exists.
     */
    @Test
    fun `the oldest account decides`() {
        val times = mapOf("a" to 1_000L, "b" to 5_000L, "c" to 3_000L)
        assertEquals(1_000L, MailboxMerge.oldestSync(times.keys.toList()) { times[it] })
    }

    /**
     * An account that has never synced makes the whole line unknown:
     * some of the mail has never been fetched, and no time describes
     * the screen.
     */
    @Test
    fun `an account that never synced makes it unknown`() {
        val times = mapOf("a" to 1_000L)
        assertNull(MailboxMerge.oldestSync(listOf("a", "b")) { times[it] })
    }

    /** No accounts is nothing to say, not "just now". */
    @Test
    fun `no accounts is nothing to say`() {
        assertNull(MailboxMerge.oldestSync(emptyList()) { 1_000L })
    }
}
