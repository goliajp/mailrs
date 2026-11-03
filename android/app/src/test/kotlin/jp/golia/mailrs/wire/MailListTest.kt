package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * What each list asks the server for.
 *
 * These are the four query parameters the handler declares, and the
 * mistakes they invite are all silent: a list that sends the wrong ones
 * still returns conversations, just not the ones the heading promises.
 */
class MailListTest {

    /**
     * **Absent, not false.** `unread=false` asks the server for threads
     * that have been *read* — a different list from "do not filter by
     * unread at all". Inbox would come back with its unread mail
     * missing, which looks like an empty morning.
     */
    @Test
    fun an_unset_flag_is_left_out_entirely() {
        val q = MailList.Inbox.axes.query()
        assertFalse("unread must not be sent by Inbox: $q", q.contains("unread"))
        assertFalse("starred must not be sent by Inbox: $q", q.contains("starred"))
    }

    /** `archived` always travels: the handler defaults it and means a boolean. */
    @Test
    fun archived_is_always_sent() {
        for (list in MailList.entries) {
            assertTrue("$list omitted archived", list.axes.query().contains("archived="))
        }
    }

    /**
     * Unread and Starred are scoped to `NonJunk`, not to everything.
     * They are attributes of a thread rather than places one lives, and
     * an unscoped version drags junk back out of the one surface it is
     * allowed to have.
     */
    @Test
    fun unread_and_starred_stay_out_of_junk() {
        assertEquals("NonJunk", MailList.Unread.axes.folder)
        assertEquals("NonJunk", MailList.Starred.axes.folder)
        assertEquals(true, MailList.Unread.axes.unread)
        assertEquals(true, MailList.Starred.axes.starred)
    }

    /** Archived carries no folder: "archived within Inbox" is not what it means. */
    @Test
    fun archived_is_cross_folder() {
        assertEquals(null, MailList.Archived.axes.folder)
        assertTrue(MailList.Archived.axes.archived)
    }

    @Test
    fun the_query_is_what_the_handler_reads() {
        assertEquals("archived=false&folder=Inbox", MailList.Inbox.axes.query())
        assertEquals("archived=false&folder=NonJunk&unread=true", MailList.Unread.axes.query())
        assertEquals("archived=false&folder=NonJunk&starred=true", MailList.Starred.axes.query())
        assertEquals("archived=true", MailList.Archived.axes.query())
    }

    /**
     * "All caught up" is wrong for Junk and alarming for Archived, so
     * each list says its own thing and none of them is blank.
     */
    @Test
    fun every_list_finishes_its_own_empty_sentence() {
        TestTexts.install()
        assertTrue(MailList.entries.none { Texts.get(it.emptyMessage).isBlank() })
        // The two that must not borrow Inbox's words. "All caught up"
        // in Junk congratulates a reader on an empty spam folder, and
        // in Archived it reads as though something was lost.
        assertEquals("No junk mail", Texts.get(MailList.Junk.emptyMessage))
        assertEquals("No archived conversations", Texts.get(MailList.Archived.emptyMessage))
    }

    /** The lists the web has and this app did not: each asks for what the web's does. */
    @Test
    fun the_new_lists_ask_for_what_the_web_asks_for() {
        assertEquals("archived=false&folder=NonJunk&pinned=true", MailList.Pinned.axes.query())
        assertEquals("archived=false&snoozed=true", MailList.Snoozed.axes.query())
        assertEquals("archived=false&folder=all", MailList.All.axes.query())
        assertEquals("archived=false&trashed=true", MailList.Trash.axes.query())
        assertEquals("archived=false&label=7", MailList.labelAxes("7").query())
        assertTrue("Review is its own endpoint", MailList.Review.axes.held)
        // The drawer lists every list but the label one, which is each label.
        assertEquals(MailList.entries.size - 1, MailList.fixed.size)
    }
}
