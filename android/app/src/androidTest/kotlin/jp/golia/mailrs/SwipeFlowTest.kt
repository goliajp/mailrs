package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
import androidx.compose.ui.test.swipeRight
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A swipe on a row does what the reader's swipe setting says, the setting
 * the web and the iPhone read too: right archives and left moves to the
 * Trash until it is changed, and both offer the undo. The same journeys as
 * the iPhone's `SwipeFlowTests`.
 */
@RunWith(AndroidJUnit4::class)
class SwipeFlowTest : MailrsUiTest() {

    private fun rows() = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size

    private fun awaiting(complaint: String, condition: () -> Boolean) {
        runCatching { compose.waitUntil(TIMEOUT_MS, condition) }
            .onFailure { throw AssertionError(complaint) }
    }

    private fun shown(text: String) = compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    /** Swipe the first row, wait for it to leave, take the undo, wait for it to come back. */
    private fun swipeAwayAndUndo(right: Boolean, done: String) {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        val before = rows()
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput {
            when (right) {
                true -> swipeRight(startX = centerX, endX = this.right)
                false -> swipeLeft(startX = centerX, endX = this.left)
            }
        }
        awaiting("the swipe did not take the row away") { rows() == before - 1 }
        awaiting("the snackbar did not say \"$done\"") { shown(done) }
        compose.onNodeWithText("Undo").performClick()
        awaiting("the undo did not bring it back; verbs the stub saw: " + readStub("/debug/verbs")) {
            rows() == before
        }
    }

    /** The default right swipe was archive here already; on iOS it marked read. */
    @Test
    fun a_swipe_right_archives_and_the_undo_brings_it_back() {
        swipeAwayAndUndo(right = true, done = "Archived")
    }

    /** It used to mark read; "Moved to Trash" is what says the Trash has it. */
    @Test
    fun a_swipe_left_moves_to_the_trash_and_the_undo_restores() {
        swipeAwayAndUndo(right = false, done = "Moved to Trash")
        assertFalse("the swipe asked as though it deleted for good", shown("Delete forever"))
    }

    /**
     * Choosing "mark read" for the right swipe is saved on the server, and
     * the next right swipe reads the row in place instead of archiving it.
     */
    @Test
    fun the_setting_changes_what_a_swipe_does() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performScrollTo().performClick()
        compose.onNodeWithTag("swipe.right").performScrollTo().performClick()
        compose.onNodeWithTag("swipe.right.Read").performClick()
        var saved = ""
        awaiting("the choice did not reach the server: $saved") {
            val all = JSONArray(readStub("/debug/settings"))
            val last = (0 until all.length()).map { all.getJSONObject(it) }
                .lastOrNull { it.getString("path") == "/api/settings/swipe" }
            saved = last?.toString().orEmpty()
            last?.getJSONObject("body")?.getString("right") == "read"
        }
        compose.onNodeWithTag("button.closeSettings").performClick()
        waitForTag("list.conversations", "the inbox did not come back")

        val before = rows()
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput {
            swipeRight(startX = centerX, endX = right)
        }
        var verbs = ""
        awaiting("the swipe never read the row on the server; verbs: $verbs") {
            verbs = readStub("/debug/verbs")
            verbs.contains("\"read ") || verbs.contains("\"unread ")
        }
        assertEquals("the swipe took the row away instead of reading it", before, rows())
        assertFalse("the swipe still archived: $verbs", verbs.contains("\"archive "))
    }
}
