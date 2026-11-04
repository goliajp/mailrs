package jp.golia.mailrs

import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The sectioned Inbox, team rows and long threads: a section shows its own
 * rows, a team conversation names who answers it, New's senders can be
 * accepted, and a thread opens with only its newest message unfolded.
 * The same journeys as the iPhone's `SectionFlowTests`.
 */
@RunWith(AndroidJUnit4::class)
class SectionFlowTest : MailrsUiTest() {

    @Test
    fun a_section_shows_its_own_rows_and_a_team_row_names_its_assignee() {
        signIn()
        waitForTag("section.reply", "no section bar over the Inbox")
        compose.onNodeWithTag("section.reply").performClick()
        waitUntilShown(hasText("Customer asks about the refund"), "the To reply section did not show its rows")
        assertTrue(
            "the team row did not say who answers it",
            compose.onAllNodes(hasTestTag("row.assignee.sec_team")).fetchSemanticsNodes().isNotEmpty(),
        )
        assertTrue(
            "a row nobody is assigned named an assignee",
            compose.onAllNodes(hasTestTag("row.assignee.sec_reply")).fetchSemanticsNodes().isEmpty(),
        )

        compose.onNodeWithTag("section.awaiting").performClick()
        waitUntilShown(hasText("Contract draft sent"), "Waiting for a reply did not show its row")
        assertTrue(
            "the last section's rows stayed on screen",
            compose.onAllNodes(hasText("Customer asks about the refund")).fetchSemanticsNodes().isEmpty(),
        )
    }

    @Test
    fun accept_all_accepts_the_shown_senders_and_they_leave_new() {
        signIn()
        waitForTag("section.new", "no section bar over the Inbox")
        compose.onNodeWithTag("section.new").performClick()
        waitUntilShown(hasText("Hello from a first contact"), "New did not show its first contacts")

        waitForTag("section.acceptAll", "New offered no Accept all")
        compose.onNodeWithTag("section.acceptAll").performClick()

        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodes(hasText("Hello from a first contact")).fetchSemanticsNodes().isEmpty()
        }
        val accepted = JSONArray(readStub("/debug/accepted"))
        val got = (0 until accepted.length()).map { accepted.getString(it) }.toSet()
        assertEquals(
            "Accept all did not send each shown sender, bare and lower-cased",
            setOf("new@first.test", "second@first.test"),
            got,
        )
    }

    @Test
    fun a_thread_opens_with_only_its_newest_message_unfolded() {
        signIn()
        waitForTag("row.conversation", "the inbox never loaded")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("message.folded.1", "the older message was not folded")
        assertTrue(
            "the newest message was folded",
            compose.onAllNodes(hasTestTag("message.header.2")).fetchSemanticsNodes().isNotEmpty(),
        )

        compose.onNodeWithTag("message.folded.1").performClick()
        waitForTag("message.header.1", "tapping a folded message did not open it")
        compose.onNodeWithTag("message.header.2").performClick()
        waitForTag("message.folded.2", "tapping the newest message's header did not fold it")
    }

    private fun waitUntilShown(matcher: androidx.compose.ui.test.SemanticsMatcher, what: String) {
        val shown = runCatching {
            compose.waitUntil(TIMEOUT_MS) { compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }
        }.isSuccess
        assertTrue(what, shown)
    }
}
