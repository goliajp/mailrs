package jp.golia.mailrs.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * When there is room for the list and a message at once.
 *
 * The number is Material's medium breakpoint. Below it a second pane is
 * a cramped column beside a cramped column; above it, a list occupying
 * a whole tablet while the message it was opened from is nowhere is a
 * phone layout stretched.
 */
class PanesTest {

    @Test
    fun a_phone_shows_one_thing_at_a_time() {
        assertFalse(Panes.twoPanes(360))
        assertFalse(Panes.twoPanes(411))
        assertFalse(Panes.twoPanes(599))
    }

    @Test
    fun a_tablet_or_an_opened_foldable_shows_two() {
        assertTrue(Panes.twoPanes(600))
        assertTrue(Panes.twoPanes(800))
        assertTrue(Panes.twoPanes(1280))
    }

    /** The list pane has to leave the detail more room than itself. */
    @Test
    fun the_list_pane_is_the_smaller_half() {
        assertTrue(Panes.LIST_PANE_WIDTH_DP < Panes.MEDIUM_WIDTH_DP / 2 + 100)
    }
}
