package jp.golia.mailrs

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Running the server, rather than administering who uses it.
 *
 * The queue, DMARC, agent keys and the sender lists: what an operator
 * opens when mail is not arriving, or is arriving from somewhere it
 * should not. Accounts, groups and permissions are the other half, in
 * `AdminFlowTest`.
 */
@RunWith(AndroidJUnit4::class)
class OperationsFlowTest : MailrsUiTest() {

    /**
     * The queue tells stuck apart from asked-for-later.
     *
     * The fixture holds one of each and a third in flight. Before the
     * row read its own timestamps the scheduled one was
     * indistinguishable from the stuck one, and a queue where every row
     * looks stuck is a queue nobody reads — so the assertion is on the
     * words beside the rows, not on how many there are.
     */
    @Test
    fun the_queue_says_which_rows_are_stuck() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Queue", "settings never listed the queue")

        compose.onNodeWithTag("admin.Queue").performClick()
        waitForTag("list.admin", "the queue never listed")

        compose.onNodeWithText("stuck@example.com").assertIsDisplayed()
        compose.onNodeWithText("attempt 3 — 421 too many connections").assertIsDisplayed()
        compose.onAllNodesWithText("scheduled for", substring = true).onFirst().assertIsDisplayed()
    }

    @Test
    fun a_dmarc_row_reads_as_passing_against_total() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Dmarc", "settings never listed DMARC")

        compose.onNodeWithTag("admin.Dmarc").performClick()
        waitForTag("list.admin", "the reports never listed")
        compose.onNodeWithText("google.com").assertIsDisplayed()
        compose.onNodeWithText("118/120 passing · p=quarantine").assertIsDisplayed()
    }

    /**
     * Who has been sending as these domains.
     *
     * A DMARC report says how much passed; this says who sent it, and
     * that is the question an operator opens the screen to answer. A
     * A source at 8 of 10 is either a forwarder breaking alignment or
     * somebody sending as the domain who should not be — and either
     * way it is invisible in the pass rate alone.
     */
    @Test
    fun the_dmarc_screen_names_the_sources() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Dmarc", "settings never listed DMARC")
        compose.onNodeWithTag("admin.Dmarc").performClick()
        waitForTag("list.admin", "the DMARC list never opened")

        // The fixture is the one the iOS suite reads too — a second
        // `/api/admin/dmarc/sources` branch written for this test sat
        // above the first and shadowed it, and the iOS assertions on
        // the older numbers went red. One stub, one set of numbers.
        compose.onNodeWithText("198.51.100.7").assertIsDisplayed()
        compose.onNodeWithText("150/150 passing", substring = true).assertIsDisplayed()
        assertEquals(
            "the failing source was not distinguished from the passing one",
            1,
            compose.onAllNodesWithText("8/10 passing · golia.jp").fetchSemanticsNodes().size,
        )
    }

    @Test
    fun agent_keys_name_what_they_can_do() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.AgentKeys", "settings never listed the keys")
        compose.onNodeWithTag("admin.AgentKeys").performClick()
        waitForTag("list.admin", "the keys never listed")

        compose.onNodeWithText("Scheduler").assertIsDisplayed()
        compose.onNodeWithText("mk_a1b2c · mail.send").assertIsDisplayed()
    }

    /**
     * A new agent key is shown once, because there is no second time.
     *
     * The server keeps a hash; the list returns a prefix. Creating one
     * and not showing the secret at that moment destroys the only copy
     * — and the app could delete keys it could not make, which is the
     * asymmetry that made this visible.
     */
    @Test
    fun a_new_agent_key_is_shown_once() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.AgentKeys", "settings never listed agent keys")
        compose.onNodeWithTag("admin.AgentKeys").performClick()
        waitForTag("list.admin", "the keys never listed")

        compose.onNodeWithTag("button.addAdminRow").performClick()
        waitForTag("field.admin0", "the form never opened")
        compose.onNodeWithTag("field.admin0").performTextInput("Nightly digest")
        compose.onNodeWithTag("field.admin1").performTextInput("mail.send, mail.read")
        compose.onNodeWithTag("button.confirmAdmin").performClick()

        waitForTag("text.newAgentKey", "the secret was never shown")
        // The whole key, not the prefix the list will carry from now on.
        compose.onNodeWithTag("text.newAgentKey").assertTextContains("mk_", substring = true)
        compose.onNodeWithTag("button.copyAgentKey").performClick()

        // And it is gone: the list has the name and the prefix.
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("Nightly digest").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onAllNodesWithTag("text.newAgentKey").assertCountEquals(0)
    }

    /**
     * The allow list reads `entries`, not `items`.
     *
     * `spam_lists.rs` answers with a different key from the admin
     * lists, and reaching for the wrong one decodes an empty list —
     * which on screen is indistinguishable from "nothing is listed".
     * So this asserts the address is there, and then that adding one
     * and removing it both reach the server.
     */
    @Test
    fun the_allow_list_loads_and_can_be_edited() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        // Past the fold: Settings has thirteen operator sections and
        // this is the eighth.
        scrollToTag("admin.Allowed", "settings never listed the allow list")
        compose.onNodeWithTag("admin.Allowed").performClick()
        waitForTag("list.admin", "the allow list never loaded")
        compose.onNodeWithText("friend@example.com").assertIsDisplayed()

        compose.onNodeWithTag("button.addAdminRow").performClick()
        waitForTag("field.admin0", "the form never opened")
        compose.onNodeWithTag("field.admin0").performTextInput("newfriend@example.com")
        compose.onNodeWithTag("button.confirmAdmin").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("newfriend@example.com").fetchSemanticsNodes().isNotEmpty()
        }

        compose.onAllNodesWithTag("button.deleteAdminRow").onFirst().performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("friend@example.com").fetchSemanticsNodes().isEmpty()
        }
    }

    /**
     * The suppression list can be emptied, and says that is what it is.
     *
     * The addresses the sender has given up on were visible and
     * permanent — the server offers only to clear the whole key, so a
     * delete on a row would have read as "stop suppressing this one"
     * and emptied all of them. A list-level action, asked first,
     * because there is no undo and every address starts being tried
     * again.
     */
    @Test
    fun the_suppression_list_can_be_cleared_after_asking() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Aliases", "settings never opened")
        compose.onNodeWithTag("admin.Suppressed").performScrollTo().performClick()
        waitForTag("list.admin", "the suppressions never listed")

        val before = compose.onAllNodesWithTag("row.admin").fetchSemanticsNodes().size
        assertTrue("the fixture suppresses nobody", before > 0)

        compose.onNodeWithTag("button.clearSuppressions").performClick()
        // Asked, not done: cancelling leaves them suppressed.
        compose.onNodeWithText("Cancel").performClick()
        assertEquals(
            "cancelling cleared them anyway",
            before,
            compose.onAllNodesWithTag("row.admin").fetchSemanticsNodes().size,
        )

        compose.onNodeWithTag("button.clearSuppressions").performClick()
        compose.onNodeWithTag("button.confirmClear").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.admin").fetchSemanticsNodes().isEmpty()
        }
    }
}
