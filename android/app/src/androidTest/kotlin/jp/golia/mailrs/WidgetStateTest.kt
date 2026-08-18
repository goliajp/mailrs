package jp.golia.mailrs

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import jp.golia.mailrs.widget.WidgetState
import jp.golia.mailrs.wire.Wire
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What the home-screen widget is given to draw.
 *
 * A widget cannot fetch — the launcher redraws it on every scroll — so
 * everything it shows comes from this snapshot. The rules are small and
 * the failures are quiet, which is why they are pinned here rather than
 * left to the drawing code.
 */
@RunWith(AndroidJUnit4::class)
class WidgetStateTest {

    private val context = ApplicationProvider.getApplicationContext<android.content.Context>()

    private fun row(id: String, unread: Int, date: Long, subject: String) = Wire.Conversation(
        threadId = id,
        subject = subject,
        participants = listOf("Alice Smith <alice@example.com>"),
        messageCount = 1,
        unreadCount = unread,
        lastDate = date,
        category = "inbox",
        flagged = false,
        snippet = "",
        pinned = false,
        archived = false,
        importanceLevel = "normal",
        importanceScore = 0f,
        requiresAction = false,
        receivedCount = 1,
        sentCount = 0,
    )

    /**
     * **Signed out is a state, not an absence.** A widget saying
     * "Nothing unread" to somebody who is signed out would be lying.
     */
    @Test
    fun a_cleared_snapshot_is_signed_out_rather_than_empty() {
        WidgetState.clear(context)
        val s = WidgetState.read(context)
        assertFalse(s.signedIn)
        assertEquals(0, s.unread)
        assertTrue(s.rows.isEmpty())
    }

    /**
     * Only unread rows, newest first, and no more than the tallest
     * widget can draw.
     *
     * The cap used to be three, which was the number the *drawing*
     * used before it learned to measure — so a widget dragged taller
     * had nothing more to show. Storing is now bounded by
     * `MAX_ROWS`; what appears is decided at draw time by the size the
     * launcher gives.
     */
    @Test
    fun it_keeps_the_newest_unread_up_to_the_cap() {
        WidgetState.write(
            context,
            signedIn = true,
            conversations = listOf(
                row("a", unread = 0, date = 500, subject = "already read"),
                row("b", unread = 1, date = 100, subject = "oldest"),
                row("c", unread = 2, date = 400, subject = "newest"),
                row("d", unread = 1, date = 300, subject = "middle"),
                row("e", unread = 1, date = 200, subject = "fourth"),
            ),
        )
        val s = WidgetState.read(context)
        assertTrue(s.signedIn)
        assertEquals("four unread threads", 4, s.unread)
        assertEquals("all four unread are kept, under the cap", 4, s.rows.size)
        assertEquals(listOf("newest", "middle", "fourth", "oldest"), s.rows.map { it.subject })
    }

    /** Signing out empties it, or the next launcher shows this account's mail. */
    @Test
    fun clearing_takes_the_mail_off_the_home_screen() {
        WidgetState.write(context, signedIn = true, conversations = listOf(row("a", 1, 1, "x")))
        assertTrue(WidgetState.read(context).rows.isNotEmpty())
        WidgetState.clear(context)
        assertTrue(WidgetState.read(context).rows.isEmpty())
    }

    /**
     * A row on the widget remembers which conversation it is.
     *
     * Three subjects on a home screen that all open the same inbox is
     * a list that names things and cannot open them. The thread id is
     * what makes the row a link, and it is stored because the widget
     * draws what was last fetched and never fetches.
     */
    @Test
    fun a_stored_row_carries_its_thread() {
        WidgetState.write(
            context,
            signedIn = true,
            conversations = listOf(
                row("t7", unread = 1, date = 300, subject = "Quarterly report"),
                row("t8", unread = 1, date = 200, subject = "Certificate renewed"),
            ),
        )
        val snapshot = WidgetState.read(context)
        assertEquals(listOf("t7", "t8"), snapshot.rows.map { it.threadId })
    }

    /**
     * The cap is real, and it is not the number of rows anybody draws.
     *
     * This is a preference blob read on every redraw, not a mailbox —
     * and the tallest widget a launcher hands out is what it has to
     * cover, nothing more.
     */
    @Test
    fun it_stores_no_more_than_the_tallest_widget_can_show() {
        WidgetState.write(
            context,
            signedIn = true,
            conversations = (1..40).map { row("t$it", unread = 1, date = it.toLong(), subject = "s$it") },
        )
        assertEquals(WidgetState.MAX_ROWS, WidgetState.read(context).rows.size)
        assertEquals("forty unread are still counted", 40, WidgetState.read(context).unread)
    }
}
