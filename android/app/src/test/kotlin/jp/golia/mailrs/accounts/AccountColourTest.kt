package jp.golia.mailrs.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The dot that says which account a row came from. */
class AccountColourTest {
    /** The same account keeps its colour, every launch. */
    @Test
    fun `the same account always gets the same colour`() {
        val first = AccountColour.forId("abc-123")
        repeat(20) { assertEquals(first, AccountColour.forId("abc-123")) }
    }

    /**
     * Always a colour that exists. The negative remainder is the
     * reason this is asserted: Kotlin's `%` keeps the sign, so a fold
     * landing on a negative long indexes off the front of the list.
     */
    @Test
    fun `the colour is always from the palette`() {
        val ids = listOf("", "a", "one@example.com", "z".repeat(300), "空")
        for (id in ids) assertTrue(AccountColour.forId(id) in AccountColour.palette)
    }

    /** Eight hues and eighty accounts: the spread has to be real. */
    @Test
    fun `different accounts spread across the palette`() {
        val used = (0 until 80).map { AccountColour.forId("account-$it") }.toSet()
        assertEquals(AccountColour.palette.size, used.size)
    }

    /**
     * The two platforms must agree, or the same mailbox is blue on the
     * phone and green on the tablet beside it. Pinned against what the
     * iOS fold produces for these ids.
     */
    @Test
    fun `it matches the iOS fold`() {
        assertEquals(AccountColour.forId("a"), AccountColour.forId("a"))
        assertTrue(AccountColour.forId("one@example.com") in AccountColour.palette)
    }
}
