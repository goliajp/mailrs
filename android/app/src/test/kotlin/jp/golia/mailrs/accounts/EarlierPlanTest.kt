package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** What to ask for when somebody wants the mail before what they have. */
class EarlierPlanTest {
    /** The range stops one below what is already held — not at it. */
    @Test
    fun `it asks for what is below the lowest held`() {
        assertEquals("801:1000", EarlierPlan.decide(1001, span = 200).range)
    }

    /** And never below uid 1, which does not exist. */
    @Test
    fun `it does not reach past the beginning`() {
        assertEquals("1:49", EarlierPlan.decide(50, span = 200).range)
    }

    /**
     * **Uid 1 held means the folder is exhausted.** There is no uid
     * below it, and asking would be a round trip whose answer is known.
     */
    @Test
    fun `holding the first message means there is nothing older`() {
        assertNull(EarlierPlan.decide(1).range)
        assertNull(EarlierPlan.decide(0).range)
    }

    /**
     * **Uids leave gaps wherever something was deleted**, so a span of
     * 200 may hold five messages. Widening is what stops somebody
     * tapping "earlier" five times to see one message.
     */
    @Test
    fun `a thin answer widens the next span`() {
        assertEquals(800, EarlierPlan.nextSpan(200, returned = 2))
        assertEquals(3200, EarlierPlan.nextSpan(800, returned = 0))
    }

    /** A full answer asked about the right amount. */
    @Test
    fun `a full answer keeps the span`() {
        assertEquals(200, EarlierPlan.nextSpan(200, returned = 200))
        assertEquals(200, EarlierPlan.nextSpan(200, returned = EarlierPlan.THIN))
    }

    /** And the widening stops, or one tap becomes its own problem. */
    @Test
    fun `the span has a ceiling`() {
        assertEquals(EarlierPlan.MAX_SPAN, EarlierPlan.nextSpan(EarlierPlan.MAX_SPAN, 0))
        assertTrue(EarlierPlan.nextSpan(2_000, 0) <= EarlierPlan.MAX_SPAN)
    }

    /**
     * **Finished is not the same as empty.** A range that is all gaps
     * returns nothing and there may be plenty below it; the folder is
     * finished when the range reached uid 1.
     */
    @Test
    fun `exhausted means the range reached the beginning`() {
        assertTrue(EarlierPlan.exhausted(EarlierPlan.decide(50, span = 200)))
        assertFalse(EarlierPlan.exhausted(EarlierPlan.decide(1001, span = 200)))
    }

    // At the ceiling the cap drops the oldest rows and this fetches
    // exactly those, so the two undo each other. Refusing is the
    // honest answer; fetching-and-discarding looks like it worked.
    @Test
    fun `a full device is asked before the network is`() {
        assertTrue(EarlierPlan.atCeiling(held = 100, ceiling = 100))
        assertTrue(EarlierPlan.atCeiling(held = 101, ceiling = 100))
        assertFalse(EarlierPlan.atCeiling(held = 99, ceiling = 100))
    }

    // The ceiling has to be far above one span, or the button works
    // once and then stops for a reason nobody can see.
    @Test
    fun `the ceiling leaves room for many spans`() {
        assertTrue(MailboxApply.PER_ACCOUNT >= EarlierPlan.FIRST_SPAN * 20)
    }
}
