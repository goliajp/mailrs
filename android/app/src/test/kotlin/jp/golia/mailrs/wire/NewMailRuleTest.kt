package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** When a periodic check should say something, and what. */
class NewMailRuleTest {

    /**
     * **The first check says nothing.** There is no "before", and
     * announcing the whole unread mailbox as new is the wrong first
     * impression — it is the backlog, not news.
     */
    @Test
    fun the_first_check_is_silent() {
        assertNull(NewMailRule.arrived(previous = null, current = 12))
        assertNull(NewMailRule.arrived(previous = null, current = 0))
    }

    @Test
    fun a_rise_is_what_arrived() {
        assertEquals(3, NewMailRule.arrived(previous = 4, current = 7))
        assertEquals(1, NewMailRule.arrived(previous = 0, current = 1))
    }

    /**
     * The count falls whenever mail is read on any device. A client
     * that notified on every change would announce another phone
     * catching up.
     */
    @Test
    fun reading_elsewhere_is_not_news() {
        assertNull(NewMailRule.arrived(previous = 7, current = 4))
        assertNull(NewMailRule.arrived(previous = 7, current = 7))
        assertNull(NewMailRule.arrived(previous = 3, current = 0))
    }

    @Test
    fun the_wording_counts_properly() {
        assertEquals("1 new message", NewMailRule.text(1))
        assertEquals("2 new messages", NewMailRule.text(2))
    }
}
