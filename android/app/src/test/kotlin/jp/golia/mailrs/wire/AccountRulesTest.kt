package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AccountRulesTest {
    /**
     * Asking about "s", "so", "som" is three requests that cannot
     * answer anything, and each one is a round trip on a phone.
     */
    @Test
    fun `a partial address is not looked up`() {
        for (v in listOf("", "s", "some", "some@", "some@x", "@x.com", " ", "a@b.")) {
            assertFalse(v, looksLikeAnAddress(v))
        }
    }

    @Test
    fun `a whole address is`() {
        assertTrue(looksLikeAnAddress("someone@qq.com"))
        assertTrue(looksLikeAnAddress("first.last@sub.uni.example"))
    }

    /** The server chooses the colour so all three clients agree. */
    @Test
    fun `a hex colour is read exactly and stays opaque`() {
        assertEquals(0xFF22C55E.toInt(), colourOf("#22c55e"))
        assertEquals(0xFF22C55E.toInt(), colourOf("22c55e"))
    }

    /**
     * A row with no dot reads as a different kind of account, and a
     * crash over a colour would be worse than either.
     */
    @Test
    fun `nonsense falls back to grey`() {
        for (junk in listOf(null, "", "#", "nope", "#12345", "#1234567", "#gggggg")) {
            assertEquals(junk.toString(), 0xFF6B7280.toInt(), colourOf(junk))
        }
    }
}
