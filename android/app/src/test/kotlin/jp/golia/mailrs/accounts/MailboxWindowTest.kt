package jp.golia.mailrs.accounts

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The two things the end of the list can offer.
 *
 * One is a `LIMIT` and costs nothing; the other is a network round
 * trip. They were one button once, and the slow one ran whenever the
 * fast one would have done.
 */
class MailboxWindowTest {
    @Test
    fun `a full window is evidence there may be more`() {
        assertTrue(MailboxWindow.moreHeld(returned = 200, asked = 200))
    }

    // The direction that matters: short is **proof** there is no more,
    // and it is what stops the list growing forever.
    @Test
    fun `a short window is proof there is not`() {
        assertFalse(MailboxWindow.moreHeld(returned = 199, asked = 200))
        assertFalse(MailboxWindow.moreHeld(returned = 0, asked = 200))
    }

    @Test
    fun `the slow action is not offered while the fast one would do`() {
        assertFalse(
            MailboxWindow.offersEarlier(moreHeld = true, shownCount = 200, searching = false),
        )
        assertTrue(
            MailboxWindow.offersEarlier(moreHeld = false, shownCount = 200, searching = false),
        )
    }

    // "Earlier" than nothing has no anchor to reach back from — the
    // ordinary pass is what gives a folder one.
    @Test
    fun `nothing to be earlier than is not offered`() {
        assertFalse(
            MailboxWindow.offersEarlier(moreHeld = false, shownCount = 0, searching = false),
        )
    }

    // A fetch against a filtered list brings back mail that will not be
    // shown, and looks like it did nothing.
    @Test
    fun `not while searching`() {
        assertFalse(
            MailboxWindow.offersEarlier(moreHeld = false, shownCount = 200, searching = true),
        )
    }
}
