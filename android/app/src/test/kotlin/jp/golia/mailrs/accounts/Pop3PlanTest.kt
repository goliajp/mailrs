package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** What to fetch from a POP3 mailbox, and what to remember. */
class Pop3PlanTest {
    private fun server(vararg pairs: Pair<Int, String>) =
        pairs.map { Pop3.Uidl(it.first, it.second) }

    /** Nothing seen yet: everything is new. */
    @Test
    fun `a first pass fetches everything`() {
        val plan = Pop3Plan.decide(server(1 to "a", 2 to "b"), emptySet())
        assertEquals(listOf(1, 2), plan.fetch)
        assertEquals(0, plan.deferred)
    }

    /** A message already seen is not fetched again. */
    @Test
    fun `what has been seen is not fetched again`() {
        val plan = Pop3Plan.decide(server(1 to "a", 2 to "b"), setOf("a"))
        assertEquals(listOf(2), plan.fetch)
        assertTrue("a" in plan.keep)
    }

    /**
     * The numbers are renumbered every session, so the same message can
     * be 3 today and 1 tomorrow. Only the uidl decides.
     */
    @Test
    fun `identity is the uidl and never the number`() {
        val plan = Pop3Plan.decide(server(1 to "b", 2 to "c"), setOf("a", "b"))
        assertEquals(listOf(2), plan.fetch)
        // "a" is gone from the server, so it goes from the set too.
        assertEquals(setOf("b"), plan.keep)
    }

    /**
     * A first sync of a mailbox with thousands of messages must not
     * download all of them before anything appears on screen — and the
     * newest are the ones somebody is looking for.
     */
    @Test
    fun `a large mailbox is fetched newest first and bounded`() {
        val all = (1..500).map { Pop3.Uidl(it, "id$it") }
        val plan = Pop3Plan.decide(all, emptySet(), limit = 100)
        assertEquals(100, plan.fetch.size)
        assertEquals(400, plan.deferred)
        // The newest hundred, and in arrival order once chosen.
        assertEquals((401..500).toList(), plan.fetch)
    }

    /**
     * The set is pruned to what the server still has, or a year of
     * bookkeeping outgrows the mailbox it is about.
     */
    @Test
    fun `ids that have gone from the server go from the set`() {
        val plan = Pop3Plan.decide(server(5 to "e"), setOf("a", "b", "c", "d", "e"))
        assertEquals(setOf("e"), plan.keep)
        assertTrue(plan.fetch.isEmpty())
    }

    /** An empty mailbox is not an error and not a crash. */
    @Test
    fun `an empty mailbox asks for nothing`() {
        val plan = Pop3Plan.decide(emptyList(), setOf("a"))
        assertTrue(plan.fetch.isEmpty())
        assertTrue(plan.keep.isEmpty())
        assertEquals(0, plan.deferred)
    }
}
