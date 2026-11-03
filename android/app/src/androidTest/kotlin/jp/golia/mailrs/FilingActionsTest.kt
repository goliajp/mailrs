package jp.golia.mailrs

import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The actions the web's table has, from Android's menus: each one sends
 * the request the web sends (asserted on what the stub was asked), the
 * ones that take a row out of the list take it, and the undo sends the
 * action that takes it back.
 */
@RunWith(AndroidJUnit4::class)
class FilingActionsTest : MailrsUiTest() {

    private fun verbsContain(entry: String, what: String) {
        var verbs = ""
        runCatching {
            compose.waitUntil(TIMEOUT_MS) {
                verbs = readStub("/debug/verbs")
                verbs.contains(entry)
            }
        }.onFailure { throw AssertionError("$what; the stub saw $verbs") }
    }

    /** Long-press the first row and pick [action] from the overflow. */
    private fun selectFirstAndPick(action: String) {
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.selectionMore", "selecting did not offer more actions")
        compose.onNodeWithTag("button.selectionMore").performClick()
        waitForTag("action.$action", "the menu did not offer $action")
        compose.onNodeWithTag("action.$action").performClick()
    }

    private fun rowCount(): Int = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size

    private fun openList(item: String) {
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag(item).performScrollTo().performClick()
    }

    @Test
    fun moving_to_the_trash_can_be_undone_and_the_undo_restores_it_on_the_server() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        val before = rowCount()
        assertTrue("the fixture has no rows", before > 0)
        selectFirstAndPick("Trash")
        compose.waitUntil(TIMEOUT_MS) { rowCount() == before - 1 }
        verbsContain("delete ", "moving to the Trash never reached the server")
        compose.waitUntil(TIMEOUT_MS) { compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Undo").performClick()
        compose.waitUntil(TIMEOUT_MS) { rowCount() == before }
        verbsContain("untrash ", "the undo did not restore it on the server")
    }

    @Test
    fun what_is_in_the_trash_is_listed_there_and_comes_back() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        selectFirstAndPick("Trash")
        verbsContain("delete ", "moving to the Trash never reached the server")
        val trashed = Regex("delete (\\S+?)\"").find(readStub("/debug/verbs"))!!.groupValues[1]
        openList("drawer.item.Trash")
        runCatching {
            compose.waitUntil(TIMEOUT_MS) {
                compose.onAllNodesWithText("In the Trash: $trashed").fetchSemanticsNodes().isNotEmpty()
            }
        }.onFailure { throw AssertionError("the Trash did not list what was put there") }
        compose.onNodeWithText("In the Trash: $trashed").performTouchInput { longClick() }
        compose.onNodeWithTag("button.selectionMore").performClick()
        waitForTag("action.Untrash", "the Trash did not offer to restore")
        compose.onNodeWithTag("action.Untrash").performClick()
        verbsContain("untrash $trashed", "restoring never reached the server")
    }

    @Test
    fun deleting_for_good_asks_and_then_wants_the_password() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        openList("drawer.item.Trash")
        waitForTag("row.conversation", "the Trash did not list")
        selectFirstAndPick("DeleteForever")
        waitForTag("button.deleteForever", "deleting for good did not ask first")
        assertTrue(
            "it was sent before the reader said yes",
            !readStub("/debug/verbs").contains("forever "),
        )
        compose.onNodeWithTag("button.deleteForever").performClick()
        // the server wants the password again, as it does on the web
        confirmPassword()
        verbsContain("forever trash1", "deleting for good never reached the server")
    }

    /** One row each, because each takes the row out of the inbox. */
    @Test
    fun the_bucket_actions_send_what_the_web_sends() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        val full = rowCount()
        for ((action, verb) in listOf(
            "Snooze" to "snooze ",
            "MarkJunk" to "mark-junk ",
            "MarkNotification" to "mark-notification ",
            "MarkPromotion" to "mark-promotion ",
        )) {
            selectFirstAndPick(action)
            compose.waitUntil(TIMEOUT_MS) { rowCount() == full - 1 }
            verbsContain(verb, "$action never reached the server")
            // the stub's inbox does not change, so a refresh puts the row
            // back and the next action has the same list to start from
            compose.onNodeWithTag("button.refresh").performClick()
            compose.waitUntil(TIMEOUT_MS) { rowCount() == full }
        }
    }

    @Test
    fun a_row_is_pinned_from_the_open_thread() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("button.threadMore", "the thread has no more-actions menu")
        compose.onNodeWithTag("button.threadMore").performClick()
        waitForTag("action.Pin", "the thread menu did not offer Pin")
        compose.onNodeWithTag("action.Pin").performClick()
        verbsContain("pin ", "pinning never reached the server")
    }

    @Test
    fun a_label_is_put_on_from_the_menu() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        selectFirstAndPick("Label")
        waitForTag("label.1", "the label picker did not list the labels")
        compose.onNodeWithTag("label.1").performClick()
        verbsContain("label ", "labelling never reached the server")
    }
}
