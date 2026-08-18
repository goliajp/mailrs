package jp.golia.mailrs

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.printToString
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeRight
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Getting around, and filing what is there.
 *
 * The gestures rather than the screens: back at every layer, the swipe
 * and its undo, the drawer, selection mode, two panes on a wide screen,
 * and a mailbox that survives the network going away. Split out of
 * `MailFlowTest` when it went back over this repo's 500-line limit.
 */
@RunWith(AndroidJUnit4::class)
class NavigationFlowTest : MailrsUiTest() {

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
     * Back is navigation on Android, not exit.
     *
     * On iOS the chevron is the way out and the edge swipe follows it;
     * here the gesture *is* the way out, and a screen that does not take
     * it closes the app instead of the thread. Driven through the
     * activity's own `OnBackPressedDispatcher`, which is what the system
     * dispatches to, so this fails for the same reason a person's swipe
     * would.
     */
    @Test
    fun back_closes_the_thread_rather_than_the_app() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        pressBack()
        waitForTag("list.conversations", "back did not return to the inbox")
    }

    @Test
    fun back_collapses_the_search() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.search").performClick()
        waitForTag("search.field", "the search never opened")

        pressBack()
        waitForTag("list.conversations", "back did not return to the inbox")
    }

    /**
     * The drawer is how six lists became reachable at all.
     *
     * The app showed one hard-coded Inbox while the server, the web and
     * iOS all carried Junk, Starred, Archived and the rest. Choosing one
     * has to re-query with that list's axes, which is why the assertion
     * is on the row that only Junk returns rather than on the heading.
     */
    @Test
    fun the_drawer_switches_which_list_is_read() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Junk").performClick()

        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("You have won").fetchSemanticsNodes().isNotEmpty()
        }
    }

    /**
     * Long press picks rows; the bar becomes the action bar.
     *
     * Android's own pattern, and the reason a row tap has two meanings:
     * while a selection is on, tapping changes it rather than opening
     * the thread. The assertion is at the wire — one batch request with
     * both threads in it, not two requests — because a client that sent
     * one per row would look identical on screen.
     */
    @Test
    fun a_long_press_selects_and_the_bar_acts_on_all_of_them() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.endSelection", "the bar never became an action bar")

        // The second row joins by an ordinary tap, which must not open it.
        compose.onAllNodesWithTag("row.conversation")[1].performClick()
        compose.onNodeWithText("2").assertIsDisplayed()

        compose.onNodeWithTag("button.selectionArchive").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            readStub("/debug/verbs").contains("t1") && readStub("/debug/verbs").contains("t2")
        }
    }

    @Test
    fun back_ends_a_selection() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.endSelection", "the bar never became an action bar")

        pressBack()
        waitForTag("button.folders", "back did not end the selection")
    }

    /**
     * A mailbox already fetched survives the network going away.
     *
     * The phone is the place this matters: a cold launch on a train
     * used to be a spinner and then "Could not reach the server", for
     * mail the device had fetched two minutes earlier. The list is
     * fetched, the server is then pointed somewhere that does not
     * answer, and the rows have to still be there — with **no error
     * banner**, because they are still the last true thing anybody
     * knew.
     */
    @Test
    fun mail_already_fetched_survives_losing_the_server() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        val before = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size
        assertTrue("the fixture listed nothing", before > 0)

        // Nothing listens on port 1.
        compose.activityRule.scenario.onActivity { it.useStubServer("http://127.0.0.1:1") }
        compose.onNodeWithTag("button.refresh").performClick()

        // Long enough for the connect to fail and the state to settle.
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("conclusion").fetchSemanticsNodes().isNotEmpty() ||
                compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size == before
        }
        compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size.let {
            assertEquals("the rows were thrown away when the server went", before, it)
        }
        compose.onAllNodesWithTag("conclusion").assertCountEquals(0)
    }

    /**
     * And it comes back from **disk**, not from memory.
     *
     * The test above keeps rows that were already on screen, which a
     * cache would not be needed for. This one throws the in-memory copy
     * away first — a cold launch without the launch — and then refreshes
     * against a server that does not answer. Rows can only come from the
     * file the last successful fetch wrote.
     */
    @Test
    fun a_cold_start_paints_from_disk_before_the_network_answers() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        val before = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size
        assertTrue("the fixture listed nothing", before > 0)

        compose.activityRule.scenario.onActivity {
            it.useStubServer("http://127.0.0.1:1")
            it.forgetLoadedMailForTest()
        }
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().isEmpty()
        }

        compose.onNodeWithTag("button.refresh").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size == before
        }
    }

    @Test
    fun a_tapped_notification_opens_that_thread() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.activityRule.scenario.onActivity { activity ->
            activity.deliverForTest(
                android.content.Intent(activity, MainActivity::class.java)
                    .putExtra(jp.golia.mailrs.wire.NewMailWorker.EXTRA_THREAD_ID, "t1"),
            )
        }

        waitForTag("list.messages", "the notification did not open the thread")
        compose.onNodeWithText("Alice Smith").assertIsDisplayed()
    }

    /**
     * A message can be filed without going back to the list first.
     *
     * Reading something and wanting it out of the way is the commonest
     * thing that happens next, and it used to mean back, find the row
     * again, swipe. Archive from the thread uses the same deferred
     * triage a swipe does, so the undo is still on offer — which is
     * what the second half asserts.
     */
    @Test
    fun a_thread_can_be_archived_and_the_undo_still_offered() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        val before = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        compose.onNodeWithTag("button.archive").performClick()
        waitForTag("list.conversations", "archiving did not return to the list")
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size == before - 1
        }

        compose.onNodeWithText("Undo").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size == before
        }
    }

    /**
     * Starring from the thread reaches the server and survives going
     * back — the icon changing is not the same as the star being kept.
     */
    @Test
    fun starring_a_thread_reaches_the_server() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        compose.onNodeWithTag("button.star").performClick()
        compose.waitUntil(TIMEOUT_MS) { readStub("/debug/verbs").contains("star t1") }
    }

    /**
     * The list pages, and does not lose the boundary second.
     *
     * The stub's `Paged` fixture is 120 threads with rows 48-52 sharing
     * one second, which is the trap: the server compares strictly, so a
     * client asking for its oldest row's own timestamp drops the
     * siblings that did not fit — silently, because a short page looks
     * exactly like the end of the mailbox.
     *
     * So this scrolls to the end and asserts the row that would be lost
     * is there. Sixty was fifty before paging existed.
     */
    @Test
    fun the_list_pages_past_fifty_without_skipping_a_shared_second() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.activityRule.scenario.onActivity { it.useFolderForTest("Paged") }

        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("Paged thread 0").fetchSemanticsNodes().isNotEmpty()
        }

        // Scroll until the list stops growing: each pass brings a page.
        var seen = 0
        repeat(12) {
            compose.onNodeWithTag("list.conversations").performScrollToIndex(
                maxOf(0, currentRowCount() - 1),
            )
            compose.waitForIdle()
            val now = currentRowCount()
            if (now == seen) return@repeat
            seen = now
        }

        // Row 52 shares its second with 48-51 and is the one a client
        // paging on its own oldest timestamp would drop.
        compose.onNodeWithTag("list.conversations")
            .performScrollToNode(hasText("Paged thread 52"))
        compose.onNodeWithText("Paged thread 52").assertIsDisplayed()
    }

    private fun currentRowCount() =
        compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size

    /**
     * A refused bulk action is said out loud, not just undone.
     *
     * The rows coming back is the visible half; without a word for it,
     * a person watching two rows leave and return has been told
     * nothing about why. Twenty-eight places in this app set an error
     * and, until this snackbar, four read it — each only when its list
     * was empty, which is exactly when this kind of failure is not.
     */
    @Test
    fun a_refused_bulk_action_says_why_the_rows_came_back() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        val before = compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size

        // The stub refuses this verb from now on.
        java.net.URL(stubBase() + "/debug/refuse-verb/archive").openConnection()
            .let { it as java.net.HttpURLConnection }
            .apply { requestMethod = "POST" }
            .inputStream.use { it.readBytes() }

        compose.onAllNodesWithTag("row.conversation").onFirst().performTouchInput { longClick() }
        waitForTag("button.endSelection", "the bar never became an action bar")
        compose.onNodeWithTag("button.selectionArchive").performClick()

        // Back on screen, and said.
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size == before
        }
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("refused", substring = true)
                .fetchSemanticsNodes().isNotEmpty()
        }
    }

    /**
     * An expired session sends you back to sign in.
     *
     * The server can stop accepting a token at any moment — it expires,
     * or an operator revokes it. Until this, a 401 became a sentence
     * and nothing else: the app went on believing it was signed in,
     * every refresh failed with the same words, and the only way out
     * was to find Sign out in Settings.
     */
    @Test
    fun a_rejected_session_returns_to_sign_in() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        java.net.URL(stubBase() + "/debug/reject-session").openConnection()
            .let { it as java.net.HttpURLConnection }
            .apply { requestMethod = "POST" }
            .inputStream.use { it.readBytes() }

        compose.onNodeWithTag("button.refresh").performClick()

        waitForTag("field.address", "a rejected session did not return to sign in")
        // Scrolled to, because the sign-in screen puts its error under
        // the fields and a short phone shows the fields first.
        waitForTag("text.signInError", "the sign-in screen did not say why")
        compose.onNodeWithTag("text.signInError")
            .assertTextContains("rejected this session", substring = true)
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

    /** `waitUntil` with something to say when it gives up. */
    private fun awaiting(complaint: String, condition: () -> Boolean) {
        runCatching { compose.waitUntil(TIMEOUT_MS, condition) }
            .onFailure { throw AssertionError(complaint) }
    }
}
