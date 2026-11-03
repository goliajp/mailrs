package jp.golia.mailrs.wire

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The wire as [MailrsClient] reads it. */
private val wireJson = kotlinx.serialization.json.Json {
    ignoreUnknownKeys = true
    explicitNulls = false
}

/**
 * Asking for the next page without losing the boundary second.
 *
 * Both rules here exist because their failures are invisible: a skipped
 * second looks like the end of the mailbox, and a page that repeats
 * looks like a list that will not stop loading.
 */
class ThreadPageTest {

    private fun row(id: String, date: Long, cursor: String? = null) = Wire.Conversation(
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
        cursor = cursor,
    )

    /** The next page starts after the last row, as the server marked it. */
    @Test
    fun the_next_page_asks_after_the_last_rows_cursor() {
        val next = ThreadPage.next(listOf(row("a", 2_000, "2000:7"), row("b", 1_000, "1000:3")))
        assertEquals(ThreadPage.Next.Cursor("1000:3"), next)
        // the colon travels encoded, as any value in a query does
        assertEquals("&cursor=1000%3A3", ThreadPage.query(next))
    }

    /**
     * **Without a cursor, one second past the oldest row.** A server that
     * stamps none still pages by `before_ts`, which compares strictly on
     * whole seconds — asking for the oldest row's own timestamp would drop
     * every sibling of that second that did not fit on the page.
     */
    @Test
    fun without_a_cursor_the_next_page_starts_one_second_past_the_oldest_row() {
        val next = ThreadPage.next(listOf(row("a", 2_000), row("b", 1_000)))
        assertEquals(ThreadPage.Next.Before(1_001L), next)
        assertEquals("&before_ts=1001", ThreadPage.query(next))
    }

    @Test
    fun an_empty_list_has_no_next_page() {
        assertNull(ThreadPage.next(emptyList()))
        assertEquals("", ThreadPage.query(null))
    }

    /** The wire carries the cursor into the row. */
    @Test
    fun a_row_decodes_its_cursor() {
        val json = """{"thread_id":"t","subject":"s","participants":[],"message_count":1,
            "unread_count":0,"last_date":5,"category":"inbox","flagged":false,"snippet":"",
            "pinned":false,"archived":false,"importance_level":"normal","importance_score":0,
            "requires_action":false,"received_count":1,"sent_count":0,"cursor":"5:42"}"""
        val decoded = wireJson.decodeFromString(Wire.Conversation.serializer(), json)
        assertEquals("5:42", decoded.cursor)
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
