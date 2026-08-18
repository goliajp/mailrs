package jp.golia.mailrs

import androidx.compose.ui.test.assertTextContains
import org.junit.Assert.assertEquals
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.longClick
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

    /**
     * A failed send can be tried again — and only that one.
     *
     * `can_resend` is the server's judgement, not a guess this side can
     * make: it reads an empty envelope reference as "the bytes are not
     * on disk" and answers 409. A button offered against that fails
     * after the tap, which is worse than not offering it. The stub's
     * fixture has one row where it is true and one where it is false,
     * so counting the buttons is the assertion.
     */
    @Test
    fun only_a_send_the_server_still_holds_can_be_sent_again() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Sent").performClick()
        waitForTag("list.sent", "the sent list never opened")

        val offers = compose.onAllNodesWithTag("button.resend").fetchSemanticsNodes().size
        assertEquals("only the failed send holds its bytes", 1, offers)

        compose.onAllNodesWithTag("button.resend").onFirst().performClick()
        // `unfiled@golia.jp` arrives percent-encoded, which is what a
        // path segment holding an address looks like on the wire.
        compose.waitUntil(TIMEOUT_MS) {
            val writes = readStub("/debug/writes")
            writes.contains("/resend") && writes.contains("unfiled")
        }
    }

    /**
     * A message can be told to leave later, and called back.
     *
     * The server has taken `scheduled_at` since scheduling existed and
     * `cancel` has been there for months with no caller anywhere,
     * because nothing could list what there was to cancel. A phone
     * that can schedule and not un-schedule is worse than one that
     * cannot schedule at all — so both halves are one test.
     */
    @Test
    fun a_message_can_be_scheduled_and_called_back() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")
        compose.onNodeWithTag("field.to").performTextInput("alice@example.com")
        compose.onNodeWithTag("field.subject").performTextInput("Not yet")

        // Long press, which is where Android puts a second meaning on a
        // primary control.
        compose.onNodeWithTag("button.send").performTouchInput { longClick() }
        waitForTag("sheet.sendTime", "long-pressing send offered no times")
        compose.onNodeWithTag("sendTime.TomorrowMorning").performClick()
        waitForTag("list.conversations", "the composer never closed after scheduling")

        val sent = readStub("/debug/sent")
        assertTrue("the send carried no scheduled_at: $sent", sent.contains("scheduled_at"))
        // In the future, and by more than the few seconds this test
        // takes: a time already passed is a 400, and the handler reads
        // anything it cannot parse as "send now".
        val at = Regex("\"scheduled_at\": (\\d+)").find(sent)?.groupValues?.get(1)?.toLong()
        assertTrue("scheduled_at was $at", at != null && at > System.currentTimeMillis() / 1000 + 3600)

        // And the other half: what is waiting can be called back.
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Sent").performClick()
        waitForTag("row.scheduled", "nothing was listed as waiting to send")
        compose.onAllNodesWithTag("button.cancelScheduled").onFirst().performClick()
        compose.waitUntil(TIMEOUT_MS) {
            readStub("/debug/writes").contains("/api/scheduled/sch1/cancel")
        }
    }

    /**
     * A failed send can be fixed and sent again.
     *
     * The other half of resend, and the half that fixes anything: a
     * resend re-enqueues the stored bytes **unchanged**, so a message
     * that failed because the address was wrong fails again. This one
     * comes back as a draft.
     *
     * Its attachments are carried, not downloaded — the bytes stay on
     * the server and the send names which to keep by index. Dropping
     * one has to send `redraft_keep` *empty* rather than omitting it:
     * absent means keep everything, and the two are opposite
     * instructions about files somebody has just removed.
     */
    @Test
    fun a_failed_send_can_be_edited_and_sent_again() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Sent").performClick()
        waitForTag("list.sent", "the sent list never opened")

        compose.onAllNodesWithTag("button.redraft").onFirst().performClick()
        waitForTag("field.to", "editing the send never opened a composer")
        compose.onNodeWithTag("field.to").assertTextContains("carol@example.com", substring = true)
        // The file it is carrying, named rather than downloaded.
        waitForTag("row.carriedAttachment", "the carried attachment was not shown")

        compose.onAllNodesWithTag("button.dropCarried").onFirst().performClick()
        compose.onNodeWithTag("field.body").performTextInput("Fixed the address. ")
        compose.onNodeWithTag("button.send").performClick()
        waitForTag("list.conversations", "the composer never closed after sending")

        val sent = readStub("/debug/sent")
        assertTrue("the send did not say what it was a re-edit of: $sent", sent.contains("unfiled"))
        // Empty, not absent: every carried file was removed, and absent
        // would put the one just dropped back on the message.
        assertTrue("dropping the file did not reach the server: $sent", sent.contains("redraft_keep"))
    }

    /**
     * What actually left, headers and all.
     *
     * The counterpart to a received message's raw view, and the thing
     * worth reading when a send failed: resend re-enqueues these exact
     * bytes, so they are the difference between "try again" and "try
     * the same mistake again".
     */
    @Test
    fun a_sent_message_can_be_read_as_it_left() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Sent").performClick()
        waitForTag("list.sent", "the sent list never opened")

        compose.onAllNodesWithTag("row.sent").onFirst().performTouchInput { longClick() }
        waitForTag("text.source", "the source never opened")
        compose.onNodeWithText("Message-ID", substring = true).assertIsDisplayed()
    }
}
