package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.longClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The lists the web has and this app lacked: each one asks for its own
 * rows, and each row is the one the stub gives only that list — so a
 * drawer entry wired to the wrong axes shows the wrong row and fails.
 */
@RunWith(AndroidJUnit4::class)
class MoreListsTest : MailrsUiTest() {

    private fun openList(item: String) {
        waitForTag("button.folders", "the list has no drawer button")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag(item).performScrollTo().performClick()
    }

    private fun shows(text: String, what: String) {
        runCatching {
            compose.waitUntil(TIMEOUT_MS) { compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
        }.onFailure { throw AssertionError("$what; the stub saw " + readStub("/debug/writes")) }
    }

    @Test
    fun each_new_list_shows_its_own_rows() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        openList("drawer.item.Pinned")
        shows("Pinned thread", "Pinned did not ask for pinned conversations")
        openList("drawer.item.Snoozed")
        shows("Snoozed thread", "Snoozed did not ask for snoozed conversations")
        openList("drawer.item.All")
        shows("Archived, still in All mail", "All mail did not include archived conversations")
        openList("drawer.item.Trash")
        shows("In the Trash: trash1", "the Trash did not ask for trashed conversations")
        openList("drawer.item.Review")
        shows("Your account will be suspended", "Review did not read the held list")
    }

    @Test
    fun a_label_is_a_list() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        // the labels are fetched when the drawer opens
        waitForTag("drawer.label.1", "the drawer did not list the labels")
        compose.onNodeWithTag("drawer.label.1").performScrollTo().performClick()
        shows("Labelled: lab1", "a label did not list its conversations")
        // and the title is the label's own name
        shows("Work", "the label list is not headed by the label")
    }

    /** Review offers to put a conversation back, and it goes to the server. */
    @Test
    fun a_held_conversation_can_be_released() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        openList("drawer.item.Review")
        shows("Your account will be suspended", "Review did not list")
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.selectionMore", "selecting did not offer more actions")
        compose.onNodeWithTag("button.selectionMore").performClick()
        waitForTag("action.Release", "Review did not offer to release")
        compose.onNodeWithTag("action.Release").performClick()
        var verbs = ""
        runCatching {
            compose.waitUntil(TIMEOUT_MS) {
                verbs = readStub("/debug/verbs")
                verbs.contains("release held1")
            }
        }.onFailure { throw AssertionError("release never reached the server; verbs $verbs") }
    }
}
