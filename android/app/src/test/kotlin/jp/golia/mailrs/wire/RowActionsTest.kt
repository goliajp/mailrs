package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The action table against the web's (`web/src/lib/actions.ts`): the
 * same pairs undo each other, only deleting for good asks, and a row
 * offers the same actions in the same order where it is shown.
 */
class RowActionsTest {
    private fun row(
        unread: Int = 0,
        flagged: Boolean = false,
        pinned: Boolean = false,
        archived: Boolean = false,
        snoozedUntil: Long? = null,
        trashedAt: Long = 0,
    ) = Wire.Conversation(
        threadId = "t1", subject = "s", participants = emptyList(), messageCount = 1,
        unreadCount = unread, lastDate = 0, category = "inbox", flagged = flagged, snippet = "",
        pinned = pinned, archived = archived, importanceLevel = "", importanceScore = 0f,
        requiresAction = false, receivedCount = 1, sentCount = 0, snoozedUntil = snoozedUntil,
        trashedAt = trashedAt,
    )

    @Test
    fun the_pairs_take_each_other_back() {
        for (a in RowAction.entries) {
            val back = a.undo ?: continue
            // Moving out of a bucket goes back by moving to the inbox,
            // which has no undo of its own; every other pair is mutual.
            if (a == RowAction.MarkNotification || a == RowAction.MarkPromotion) {
                assertEquals(RowAction.MoveToInbox, back)
                continue
            }
            assertEquals("$a <-> $back", a, back.undo)
        }
        assertEquals(null, RowAction.DeleteForever.undo)
    }

    @Test
    fun only_deleting_for_good_asks_first() {
        assertEquals(listOf(RowAction.DeleteForever), RowAction.entries.filter { it.confirm })
    }

    @Test
    fun the_batch_verbs_are_the_ones_the_route_knows() {
        // crates/webapi/src/handlers/conversation_verbs.rs, batch_mutation
        val known = setOf("read", "unread", "star", "unstar", "archive", "unarchive", "delete", "untrash", "label", "unlabel")
        assertTrue(RowAction.entries.mapNotNull { it.batch }.all { it in known })
        assertEquals("delete", RowAction.Trash.batch)
    }

    @Test
    fun a_row_offers_what_the_web_offers_in_the_same_order() {
        val now = 1_000L
        assertEquals(
            listOf(
                RowAction.Read, RowAction.Star, RowAction.Pin, RowAction.Archive, RowAction.Snooze,
                RowAction.MarkNotification, RowAction.MarkPromotion, RowAction.MarkJunk,
                RowAction.Label, RowAction.Trash,
            ),
            RowActions.forRow(row(unread = 1), MailList.Inbox, now),
        )
        val done = RowActions.forRow(row(flagged = true, pinned = true, archived = true, snoozedUntil = 2_000), MailList.Inbox, now)
        assertTrue(done.containsAll(listOf(RowAction.Unread, RowAction.Unstar, RowAction.Unpin, RowAction.Unarchive, RowAction.Unsnooze)))
        // A snooze that has passed is no snooze.
        assertTrue(RowAction.Snooze in RowActions.forRow(row(snoozedUntil = 500), MailList.Inbox, now))
    }

    @Test
    fun each_bucket_offers_its_own_way_out() {
        val now = 0L
        assertTrue(RowAction.MarkNotJunk in RowActions.forRow(row(), MailList.Junk, now))
        assertFalse(RowAction.MarkJunk in RowActions.forRow(row(), MailList.Junk, now))
        val np = RowActions.forRow(row(), MailList.NP, now)
        assertTrue(RowAction.MoveToInbox in np && RowAction.MarkJunk in np)
    }

    @Test
    fun in_the_trash_a_conversation_comes_back_or_goes_for_good() {
        val inTrash = listOf(RowAction.Unread, RowAction.Untrash, RowAction.DeleteForever)
        assertEquals(inTrash, RowActions.forRow(row(trashedAt = 5), MailList.Trash, 0))
        // wherever it is seen
        assertEquals(inTrash, RowActions.forRow(row(trashedAt = 5), MailList.All, 0))
        assertEquals(listOf(RowAction.Untrash, RowAction.DeleteForever), RowActions.forSelection(MailList.Trash))
    }

    @Test
    fun review_offers_to_release() {
        assertEquals(RowAction.Release, RowActions.forRow(row(), MailList.Review, 0).first())
        assertEquals(RowAction.Release, RowActions.forSelection(MailList.Review).first())
    }

    @Test
    fun a_label_list_offers_to_take_the_label_off() {
        assertTrue(RowAction.Unlabel in RowActions.forRow(row(), MailList.Label, 0))
        assertFalse(RowAction.Unlabel in RowActions.forRow(row(), MailList.Inbox, 0))
    }
}
