package jp.golia.mailrs.wire

import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The swipe setting: its choices and default against `actions.json`'s
 * `swipe` block (the server is checked against the same block), and the
 * action each choice runs on a row.
 */
class SwipeSettingTest {
    private fun row(unread: Int = 0, archived: Boolean = false, trashedAt: Long = 0) = Wire.Conversation(
        threadId = "t1", subject = "s", participants = emptyList(), messageCount = 1,
        unreadCount = unread, lastDate = 0, category = "inbox", flagged = false, snippet = "",
        pinned = false, archived = archived, importanceLevel = "", importanceScore = 0f,
        requiresAction = false, receivedCount = 1, sentCount = 0, snoozedUntil = null,
        trashedAt = trashedAt,
    )

    private val block by lazy {
        Json.parseToJsonElement(File("src/main/assets/actions.json").readText()).jsonObject.getValue("swipe").jsonObject
    }

    private fun wireName(c: SwipeChoice) = Json.encodeToString(SwipeChoice.serializer(), c).trim('"')

    @Test
    fun the_choices_and_the_default_are_the_shared_tables() {
        assertEquals(
            block.getValue("choices").jsonArray.map { it.jsonPrimitive.content },
            SwipeChoice.entries.map(::wireName),
        )
        val default = block.getValue("default").jsonObject
        assertEquals(default.getValue("left").jsonPrimitive.content, wireName(SwipeSetting.Standard.left))
        assertEquals(default.getValue("right").jsonPrimitive.content, wireName(SwipeSetting.Standard.right))
    }

    @Test
    fun the_default_archives_right_and_trashes_left() {
        assertEquals(RowAction.Archive, SwipeSetting.Standard.right.action(row(), MailList.Inbox))
        assertEquals(RowAction.Trash, SwipeSetting.Standard.left.action(row(), MailList.Inbox))
    }

    @Test
    fun each_choice_runs_the_action_that_applies() {
        assertEquals(RowAction.Unarchive, SwipeChoice.Archive.action(row(archived = true), MailList.Archived))
        assertEquals(RowAction.Read, SwipeChoice.Read.action(row(unread = 2), MailList.Inbox))
        assertEquals(RowAction.Unread, SwipeChoice.Read.action(row(), MailList.Inbox))
        assertNull(SwipeChoice.None.action(row(), MailList.Inbox))
    }

    /** Nothing a swipe runs deletes for good. */
    @Test
    fun in_the_trash_both_restore() {
        for (choice in listOf(SwipeChoice.Archive, SwipeChoice.Trash)) {
            assertEquals(RowAction.Untrash, choice.action(row(trashedAt = 5), MailList.Trash))
            assertEquals(RowAction.Untrash, choice.action(row(), MailList.Trash))
        }
        for (choice in SwipeChoice.entries) {
            assertNotEquals(RowAction.DeleteForever, choice.action(row(trashedAt = 5), MailList.Trash))
        }
    }

    @Test
    fun the_wire_names_are_the_servers() {
        val s = Json.decodeFromString(SwipeSetting.serializer(), """{"left":"none","right":"read"}""")
        assertEquals(SwipeSetting(left = SwipeChoice.None, right = SwipeChoice.Read), s)
    }
}
