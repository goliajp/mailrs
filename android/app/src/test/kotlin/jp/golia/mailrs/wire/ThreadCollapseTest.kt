package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same cases as the iPhone's `ThreadCollapseTests`. */
class ThreadCollapseTest {
    @Test
    fun onlyTheLastStartsExpanded() {
        assertTrue(ThreadCollapse.isExpanded(uid = 2, lastUid = 2, toggled = emptySet()))
        assertFalse(ThreadCollapse.isExpanded(uid = 1, lastUid = 2, toggled = emptySet()))
    }

    @Test
    fun togglingInvertsEitherDirection() {
        assertTrue(ThreadCollapse.isExpanded(uid = 1, lastUid = 2, toggled = setOf(1)))
        assertFalse(ThreadCollapse.isExpanded(uid = 2, lastUid = 2, toggled = setOf(2)))
    }

    @Test
    fun aSingleMessageThreadIsExpanded() {
        assertTrue(ThreadCollapse.isExpanded(uid = 7, lastUid = 7, toggled = emptySet()))
    }

    @Test
    fun snippetsFoldWhitespaceAndBound() {
        assertEquals("a b c", ThreadCollapse.snippet("  a\n\n  b\tc  "))
        assertEquals("", ThreadCollapse.snippet(null))
        val cut = ThreadCollapse.snippet("x".repeat(200))
        assertEquals(81, cut.length)
        assertTrue(cut.endsWith("…"))
    }
}
