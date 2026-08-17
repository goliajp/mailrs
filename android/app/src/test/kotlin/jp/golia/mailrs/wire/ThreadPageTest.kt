package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Asking for the next page without losing the boundary second.
 *
 * Both rules here exist because their failures are invisible: a skipped
 * second looks like the end of the mailbox, and a page that repeats
 * looks like a list that will not stop loading.
 */
class ThreadPageTest {

    private fun row(id: String, date: Long) = Wire.Conversation(
        threadId = id,
        subject = id,
        participants = listOf("a@x.test"),
        messageCount = 1,
        unreadCount = 0,
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
     * **One second past the oldest row, not the oldest row.** The
     * server compares strictly, and `last_date` is whole seconds — so
     * asking for the oldest row's own timestamp drops every sibling of
     * that second that did not fit on the page.
     */
    @Test
    fun the_next_page_starts_one_second_past_the_oldest_row() {
        assertEquals(1_001L, ThreadPage.nextBefore(listOf(row("a", 2_000), row("b", 1_000))))
    }

    @Test
    fun an_empty_list_has_no_next_page() {
        assertNull(ThreadPage.nextBefore(emptyList()))
    }

    /** The overlap that rule buys is dropped here, by id. */
    @Test
    fun merging_drops_rows_already_held() {
        val held = listOf(row("a", 3_000), row("b", 2_000))
        val merged = ThreadPage.merge(held, listOf(row("b", 2_000), row("c", 1_000)))
        assertEquals(listOf("a", "b", "c"), merged.rows.map { it.threadId })
        assertTrue(merged.progressed)
    }

    /**
     * **A page with nothing new is the end.** Stopping on "was it a
     * full page?" would ask for the same boundary second forever, since
     * that second is deliberately re-requested.
     */
    @Test
    fun a_page_of_rows_already_held_is_the_end() {
        val held = listOf(row("a", 1_000), row("b", 1_000))
        val merged = ThreadPage.merge(held, listOf(row("a", 1_000), row("b", 1_000)))
        assertFalse(merged.progressed)
        assertEquals(2, merged.rows.size)
    }
}
