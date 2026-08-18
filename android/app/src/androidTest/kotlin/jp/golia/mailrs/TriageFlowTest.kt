package jp.golia.mailrs

import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeRight
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Filing mail by swiping, and taking it back.
 *
 * Its own file: the undo is a small state machine with a clock in it —
 * a window, a banner bounded by that window, a commit at the end and a
 * restore if the server refuses — and it is easier to see in one
 * place than scattered through the navigation tests.
 */
@RunWith(AndroidJUnit4::class)
class TriageFlowTest : MailrsUiTest() {

    /**
     * Swipe a row away and put it back.
     *
     * The undo is the only protection this screen offers — no dialog
     * asks — so a swipe that could not be undone would make every
     * mis-swipe permanent. That the row **returns** is the assertion;
     * that it left is only half of it.
     */
    @Test
    fun a_swiped_row_can_be_brought_back() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        val before = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size
        assertTrue("the fixture has no rows to swipe", before > 0)

        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput {
            swipeRight(startX = centerX, endX = right)
        }
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size == before - 1
        }

        // The snackbar's action is the platform's control, so this taps
        // the word rather than calling the view model — the test has to
        // fail if the snackbar stops being wired to it.
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithText("Undo").performClick()
        try {
            compose.waitUntil(TIMEOUT_MS) {
                compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size == before
            }
        } catch (e: Throwable) {
            throw AssertionError(
                // The verbs are in the message because they say which
                // of the two failures this is: none means the undo did
                // not restore, one means the row was filed anyway.
                "the row did not come back. verbs the stub saw: " + readStub("/debug/verbs") +
                    "; rows now " + compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size +
                    ", was " + before,
                e,
            )
        }
    }

    /**
     * Leaving the app commits what was swiped away.
     *
     * The undo window holds the request for five seconds, and a person
     * who swipes and then leaves is gone long before that — so the
     * archive was never sent, and the row was back next time they
     * looked. Watching a message leave and finding it returned is
     * exactly the kind of thing that makes a mail client feel
     * untrustworthy.
     */
    @Test
    fun leaving_the_app_commits_a_swipe() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput {
            swipeRight(startX = centerX, endX = right)
        }
        awaiting("the swipe never offered an undo") {
            compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty()
        }

        // Away, well inside the five seconds.
        compose.activityRule.scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)

        var writes = ""
        awaiting("leaving the app never sent the archive; the stub saw <$writes>") {
            writes = readStub("/debug/writes")
            writes.contains("conversations/batch")
        }
    }

    /**
     * The undo lasts as long as the undo lasts.
     *
     * Material's `Short` is four seconds and the window before the
     * archive is committed is five, so there was a second in which the
     * action could still be taken back and nothing on screen offered
     * to. Two numbers describing one fact; now the banner is bounded
     * by the window itself.
     */
    @Test
    fun the_undo_is_offered_for_as_long_as_it_is_possible() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput {
            swipeRight(startX = centerX, endX = right)
        }
        awaiting("the swipe never offered an undo") {
            compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty()
        }

        // Past four seconds, where the banner used to have gone, and
        // still inside the five the app waits before committing.
        compose.mainClock.advanceTimeBy(4_500)
        compose.waitForIdle()
        assertTrue(
            "the undo went before the window did",
            compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty(),
        )
    }

    /** `waitUntil` with something to say when it gives up. */
    private fun awaiting(complaint: String, condition: () -> Boolean) {
        runCatching { compose.waitUntil(TIMEOUT_MS, condition) }
            .onFailure { throw AssertionError(complaint) }
    }
}
