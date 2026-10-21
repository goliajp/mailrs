package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Marking a thread unread is not filing it: back on the list the row is
 * still there, and there is nothing to undo.
 */
@RunWith(AndroidJUnit4::class)
class MarkUnreadTest : MailrsUiTest() {
    private fun rows() = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size

    @Test
    fun a_thread_marked_unread_stays_in_the_list() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        val before = rows()
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        compose.onNodeWithTag("button.markUnread").performClick()
        waitForTag("list.conversations", "marking unread did not return to the list")
        // a row that leaves goes at once; give it the time it would take
        val left = runCatching { compose.waitUntil(2_000) { rows() < before } }.isSuccess
        assertEquals("the row left the list", false, left)
        assertEquals("an undo was offered", 0, compose.onAllNodesWithText("Undo").fetchSemanticsNodes().size)
    }
}
