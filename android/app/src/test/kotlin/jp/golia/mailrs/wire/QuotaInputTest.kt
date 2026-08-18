package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class QuotaInputTest {
    @Test
    fun gigabytes_are_decimal_so_the_row_reads_back_what_was_typed() {
        assertEquals(2_000_000_000L, QuotaInput.parse("2"))
        assertEquals(500_000_000L, QuotaInput.parse("0.5"))
        assertEquals("2", QuotaInput.display(2_000_000_000L))
        assertEquals("0.5", QuotaInput.display(500_000_000L))
    }

    /**
     * Clearing the field lifts the cap. The alternative — a separate
     * "No limit" gesture — leaves an empty field meaning nothing, and
     * an operator who cleared it to remove a limit would find it still
     * there.
     */
    @Test
    fun empty_and_zero_both_mean_no_limit() {
        assertEquals(0L, QuotaInput.parse(""))
        assertEquals(0L, QuotaInput.parse("   "))
        assertEquals(0L, QuotaInput.parse("0"))
        assertEquals("", QuotaInput.display(0))
        assertEquals("", QuotaInput.display(null))
    }

    /**
     * A field that cannot be read is not sent. The dialog stays open
     * on null, which is the whole reason this returns one rather than
     * falling back to zero — falling back would lift the cap on a
     * typo.
     */
    @Test
    fun what_is_not_a_number_is_not_a_quota() {
        assertNull(QuotaInput.parse("two"))
        assertNull(QuotaInput.parse("-1"))
        assertNull(QuotaInput.parse("1 GB"))
    }
}
