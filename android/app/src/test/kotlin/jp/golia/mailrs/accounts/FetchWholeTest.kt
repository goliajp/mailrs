package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Whether to fetch a whole message or only its beginning. */
class FetchWholeTest {
    /** An ordinary message is fetched whole, which is nearly all of them. */
    @Test
    fun `a small message is fetched whole`() {
        assertEquals(FetchWhole.Plan.Whole, FetchWhole.decide(12_000))
        assertEquals(FetchWhole.Plan.Whole, FetchWhole.decide(FetchWhole.THRESHOLD))
    }

    /**
     * A message with a 25 MB attachment is 25 MB to fetch, and
     * fetching it to show two lines of text — on somebody's mobile
     * data, without asking — is noticed on a bill rather than on a
     * screen.
     */
    @Test
    fun `a large message is only begun`() {
        val plan = FetchWhole.decide(25_000_000)
        assertTrue(plan.toString(), plan is FetchWhole.Plan.Beginning)
        assertEquals(FetchWhole.PREVIEW, (plan as FetchWhole.Plan.Beginning).bytes)
    }

    /** And all of it once the reader has asked. */
    @Test
    fun `asking for all of it gets all of it`() {
        assertEquals(FetchWhole.Plan.Whole, FetchWhole.decide(25_000_000, askedForAll = true))
    }

    /**
     * **A message of unknown size is fetched whole.** It is usually a
     * small one, and refusing to show it properly on a guess is worse
     * than the fetch.
     */
    @Test
    fun `an unknown size is not treated as large`() {
        assertEquals(FetchWhole.Plan.Whole, FetchWhole.decide(null))
    }

    /**
     * `<0.262144>` is RFC 3501's partial fetch: offset then length.
     * The offset is written even though it is zero, because the form
     * without it means something else — the whole body.
     */
    @Test
    fun `the partial form carries an offset`() {
        assertEquals("BODY.PEEK[]", FetchWhole.bodyItem(FetchWhole.Plan.Whole))
        assertEquals(
            "BODY.PEEK[]<0.262144>",
            FetchWhole.bodyItem(FetchWhole.Plan.Beginning(262_144)),
        )
    }
}
