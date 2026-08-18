package jp.golia.mailrs

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What was sent, and whether it arrived.
 *
 * Its own file rather than a corner of the composer's: sending a
 * message and reviewing what left are different questions, and the two
 * endpoints behind this one are joined by a rule with its own unit
 * tests (`SendJoinTest`).
 */
@RunWith(AndroidJUnit4::class)
class SentFlowTest : MailrsUiTest() {

    /**
     * What was sent, and whether it arrived.
     *
     * iOS and the web have had this screen for as long as they have
     * had a Send button; this app did not, which meant the only way to
     * find out whether a message left was to look in the thread and
     * hope. Two endpoints joined on Message-ID, and the row that
     * predates the delivery projection must say nothing rather than
     * claim it arrived.
     */
    @Test
    fun the_sent_list_shows_what_left_and_what_became_of_it() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Sent").performClick()
        waitForTag("list.sent", "the sent list never opened")

        compose.onNodeWithText("Filed and delivered").assertIsDisplayed()
        compose.onNodeWithText("delivered").assertIsDisplayed()

        // A send the sweep has not filed yet is still on the list —
        // without that pass, a message that left successfully is absent
        // from the only screen that would show it.
        compose.onNodeWithText("Never left the queue").assertIsDisplayed()

        // And the one nobody tracked carries no status at all. Counting
        // the badges is the assertion: three rows, and only the two the
        // projection knows about are labelled.
        val badges = compose.onAllNodesWithTag("text.sendStatus").fetchSemanticsNodes().size
        val rows = compose.onAllNodesWithTag("row.sent").fetchSemanticsNodes().size
        assertTrue("every row claimed a delivery status: $badges of $rows", badges < rows)
    }
}
