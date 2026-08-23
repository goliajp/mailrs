package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Turning a list of uids into something a server will accept. */
class UidRangesTest {
    /** Consecutive uids are a range, which is what they nearly always are. */
    @Test
    fun `runs collapse`() {
        assertEquals("1:3,7:8,20", UidRanges.collapse(listOf(1L, 2L, 3L, 7L, 8L, 20L)))
        assertEquals("5", UidRanges.collapse(listOf(5L)))
        assertEquals("", UidRanges.collapse(emptyList()))
    }

    /**
     * Sorted first: a set has no order, and a server reading `20,1:3`
     * gets a valid but pointlessly awkward sequence.
     */
    @Test
    fun `order does not matter to the caller`() {
        assertEquals("1:3,20", UidRanges.collapse(setOf(20L, 2L, 1L, 3L)))
        // And a repeat is not two uids.
        assertEquals("1:2", UidRanges.collapse(listOf(1L, 2L, 2L, 1L)))
    }

    /**
     * **The mailbox that most needs its flags refreshed is the one
     * where a naive command stops working**: five thousand uids named
     * one by one is a line tens of kilobytes long, and servers refuse
     * over-long lines.
     */
    @Test
    fun `a long sparse list is split into commands a server will take`() {
        // Every other uid, so nothing collapses.
        val sparse = (1L..4000L step 2).toList()
        val batches = UidRanges.batches(sparse)
        assertTrue("nothing was split", batches.size > 1)
        for (batch in batches) assertTrue(batch.length.toString(), batch.length <= UidRanges.MAX_CHARS)
        // Nothing was lost or invented.
        val rejoined = batches.joinToString(",")
        assertEquals(UidRanges.collapse(sparse), rejoined)
    }

    /**
     * **Split on whole runs, never inside one.** Half of `1:3` is not
     * a range, and a server would read whatever the halves happen to
     * spell.
     */
    @Test
    fun `a range is never cut in half`() {
        val batches = UidRanges.batches((1L..4000L step 2).toList(), maxChars = 40)
        for (batch in batches) {
            assertTrue(batch, !batch.startsWith(":") && !batch.endsWith(":"))
            for (run in batch.split(',')) {
                val parts = run.split(':')
                assertTrue(run, parts.size <= 2 && parts.all { it.isNotEmpty() })
            }
        }
    }

    /** A short list is one command, and an empty one is no command. */
    @Test
    fun `short and empty`() {
        assertEquals(listOf("1:10"), UidRanges.batches((1L..10L).toList()))
        assertEquals(emptyList<String>(), UidRanges.batches(emptyList()))
    }
}
