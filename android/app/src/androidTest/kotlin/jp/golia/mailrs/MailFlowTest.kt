package jp.golia.mailrs

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
 * The path a person actually takes: sign in, see the inbox, open a
 * thread, come back.
 *
 * **Against the same stub the iOS suite drives** —
 * `ios/Testing/stub-api.py` on port 6039, reached from the emulator as
 * `10.0.2.2`. Two clients against one stub is the point: a stub written
 * for Android would be a second opinion about what the Rust handlers
 * send, and the whole reason that file exists is that a client model
 * which disagrees with the server should fail here rather than in
 * somebody's inbox.
 *
 * A test that needs someone's real password is a test nobody runs, so
 * the stub takes any password and the suite is about what the app does
 * afterwards.
 *
 * `scripts/android-build.sh` starts and stops the stub around this, the
 * way `ios-build.sh` does for the other one.
 */
@RunWith(AndroidJUnit4::class)
class MailFlowTest {

    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    /**
     * Every test starts signed out.
     *
     * The token store survives the process, so without this the second
     * run of the suite opens on the inbox and the sign-in tests fail
     * looking for fields that are not there — a stale session wearing
     * the costume of a broken locator.
     */
    /**
     * The stub keeps what it was sent until told otherwise, so a run
     * that asserts on `/debug/sent` can be reading the previous run's
     * message. The iOS suite resets it in `launch()` for the same
     * reason; skipping it is how a passing assertion stops meaning
     * anything.
     */
    @Before
    fun resetStub() {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL") ?: DEFAULT_STUB
        val c = java.net.URL("$stub/debug/reset").openConnection() as java.net.HttpURLConnection
        c.requestMethod = "POST"
        c.connectTimeout = 5_000
        c.readTimeout = 5_000
        c.inputStream.use { it.readBytes() }
    }

    @Before
    fun startSignedOut() {
        compose.activityRule.scenario.onActivity { it.signOutForTest() }
        // Displayed, not merely present. Signing out slides the sign-in
        // screen back in, and a field that exists but is still off the
        // left edge takes a click at a coordinate that is not on it.
        // One run in fourteen failed here, filled fields and all.
        waitForTag("field.address", "the sign-in screen never came back")
    }

    /** The activity is launched by the rule; point it at the stub. */
    private fun signIn(address: String = "me@golia.jp", password: String = "anything") {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL")
            ?: DEFAULT_STUB
        compose.activityRule.scenario.onActivity { it.useStubServer(stub) }

        compose.onNodeWithTag("field.address").performTextInput(address)
        compose.onNodeWithTag("field.password").performTextInput(password)
        compose.onNodeWithTag("button.signIn").performClick()
    }

    /**
     * Compose's idling waits for recomposition, not for a network call
     * on `Dispatchers.IO` — so a bare assertion after a click races the
     * response. Every wait here is bounded and says what it was waiting
     * for, because "test timed out" names nothing.
     */
    /**
     * Wait until something is **on screen**, not until it exists.
     *
     * These were two different checks: it waited for the node to appear
     * and then asserted, once, that it was displayed. A node that exists
     * but is still sliding or expanding fails that single assertion, so
     * the helper was sound only as long as nothing animated. Adding
     * screen transitions turned two search tests red without touching
     * search — the failure was in the waiting, not the app.
     */
    private fun waitForTag(tag: String, what: String) {
        try {
            compose.waitUntil(TIMEOUT_MS) {
                compose.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty() &&
                    runCatching { compose.onAllNodesWithTag(tag).onFirst().assertIsDisplayed() }.isSuccess
            }
        } catch (e: Throwable) {
            throw AssertionError(
                what + "\n" + compose.onRoot().printToString(maxDepth = 12).take(2500),
                e,
            )
        }
    }

    @Test
    fun signing_in_lists_the_inbox() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
    }

    @Test
    fun opening_a_conversation_shows_its_messages() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        // And back, because a screen you cannot leave is not a screen.
        compose.onNodeWithTag("button.back").performClick()
        waitForTag("list.conversations", "back did not return to the inbox")
    }

    /**
     * A wrong server is the failure a person actually hits first, and
     * the app must say so rather than sitting on an empty list. The
     * unreachable port is deliberate: nothing listens there, so this
     * tests the message and not a 500.
     */
    @Test
    fun an_unreachable_server_says_so() {
        compose.activityRule.scenario.onActivity { it.useStubServer("http://10.0.2.2:1") }
        compose.onNodeWithTag("field.address").performTextInput("me@golia.jp")
        compose.onNodeWithTag("field.password").performTextInput("anything")
        compose.onNodeWithTag("button.signIn").performClick()

        waitForTag("text.signInError", "no error was shown for an unreachable server")
    }

    /**
     * The server field is prefilled but editable — someone else's
     * deployment is the ordinary case for a self-hosted mail server.
     */
    @Test
    fun the_server_field_can_be_changed() {
        compose.onNodeWithTag("field.server").performTextClearance()
        compose.onNodeWithTag("field.server").performTextInput("mail.example.test")
        compose.onNodeWithTag("field.server").assertIsDisplayed()
    }

    /**
     * Reply, send, and read back what the stub actually received.
     *
     * The two fields that decide whether a reply is a reply —
     * `in_reply_to` and the recipient — are not on screen anywhere, so
     * asserting the composer closed would prove only that a button
     * worked. `/debug/sent` is the stub's record of what arrived, which
     * is the same thing the iOS suite reads for the same reason.
     */
    @Test
    fun replying_sends_a_reply_and_not_a_new_message() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        compose.onAllNodesWithTag("button.reply").onFirst().performClick()
        waitForTag("field.body", "the composer never opened")

        // The recipients and the quoted history are already filled by
        // `ReplyRecipients`; a person types their answer above it.
        compose.onNodeWithTag("field.body").performTextInput("Thanks — noted.")
        compose.onNodeWithTag("button.send").performClick()

        // Wait for the thread to be back — a positive signal. Waiting
        // for the composer to *disappear* is satisfied by anything that
        // removes it, including a crash, and says nothing about where
        // the app went.
        waitForTag("list.messages", "the composer did not return to the thread after sending")

        val sent = readStub("/debug/sent")
        assertTrue("the stub received no message at all: $sent", sent.contains("Thanks"))
        assertTrue("the reply did not go to the sender of the message: $sent",
            sent.contains("alice@example.com"))
        assertTrue("the reply carried no in_reply_to, so it starts a new thread: $sent",
            sent.contains("in_reply_to") && !sent.contains("\"in_reply_to\": null"))
    }

    /**
     * A To line that looks filled and names nobody leaves the send
     * button disabled.
     *
     * The first version of this test clicked send and waited for an
     * error, which could never arrive: the button was already disabled,
     * so the click did nothing and the wait timed out. That is the test
     * finding a real disagreement — the button asked `isNotBlank()` and
     * the send asked "does this name anyone", and `"   "` answers those
     * differently. One rule now, and this asserts the visible half.
     */
    @Test
    fun a_message_with_no_recipient_cannot_be_sent() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.body", "the composer never opened")

        // `button.send` is disabled with an empty To, which is the
        // first defence; the message below is the second.
        compose.onNodeWithTag("field.to").performTextInput(" , ; ")
        compose.onNodeWithTag("button.send").assertIsNotEnabled()

        // And it becomes sendable the moment it names somebody.
        compose.onNodeWithTag("field.to").performTextInput("a@x.test")
        compose.onNodeWithTag("button.send").assertIsEnabled()
    }

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
     * Search, and in the order the server ranked it.
     *
     * Both fixtures carry "ref 2026" and the stub returns the **older**
     * one first, because the endpoint hydrates ranked hit ids rather
     * than dates. A client that re-sorts by date would put the newer
     * thread on top and look perfectly reasonable doing it, so the
     * assertion is on which row is first, not on how many there are.
     */
    @Test
    fun search_keeps_the_servers_ranking() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.onNodeWithTag("button.search").performClick()
        compose.onNodeWithTag("search.field").performTextInput("ref 2026")
        waitForTag("list.searchResults", "the search never returned anything")
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("row.conversation").fetchSemanticsNodes().size == 2
        }

        compose.onAllNodesWithTag("row.conversation")[0]
            .assertTextContains("請求書のご送付につきまして")
    }

    /** A term nothing matches says so, naming the term. */
    @Test
    fun a_search_with_no_hits_says_which_term_missed() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.onNodeWithTag("button.search").performClick()
        compose.onNodeWithTag("search.field").performTextInput("zzzznothing")
        waitForTag("search.empty", "an empty search never reported itself")
        compose.onNodeWithTag("search.empty").assertTextContains("zzzznothing", substring = true)
    }

    /**
     * A message body is rendered, and it fetches nothing until asked.
     *
     * Two claims in one flow because they are one behaviour: the body
     * is HTML in a WebView — the fixture's plain part is the two words
     * "plain fallback" against a newsletter, so a client preferring it
     * shows a different message than the sender composed — and the
     * remote image in it stays unfetched behind a banner, because
     * fetching is what tells the sender the message was opened.
     */
    @Test
    fun a_body_renders_and_holds_its_remote_content() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        // Height, not presence. A `WebView` that measured to nothing
        // still puts a semantics node on the tree, so "the node exists"
        // passes for a body nobody can read — which is the failure this
        // is most likely to have.
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("body.web").fetchSemanticsNodes()
                .any { it.size.height > 100 }
        }
        waitForTag("body.remoteBlocked", "the remote image was not held back")

        compose.onNodeWithTag("button.loadImages").performClick()
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("body.remoteBlocked").fetchSemanticsNodes().isEmpty()
        }
    }

    /**
     * An attachment is fetched **by its own index**.
     *
     * The fixture's two files preview identically, so a client that
     * always asked the server for index 0 would show the right name
     * over the wrong bytes and look correct doing it. `/debug/fetched`
     * is the only thing that can tell those apart, which is why the
     * second row is the one tapped.
     */
    @Test
    fun the_second_attachment_is_the_one_fetched() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.attachments", "the message listed no attachments")

        val rows = compose.onAllNodesWithTag("row.attachment").fetchSemanticsNodes().size
        assertEquals("the fixture offers two attachments", 2, rows)

        // Existence is not reach. The rows sit under a rendered body
        // that is often taller than the screen, so the second one can be
        // below the fold — and a tap at a coordinate that is off-screen
        // is not the tap this test means. iOS's copy of this test says
        // the same thing about the unsubscribe button.
        compose.onAllNodesWithTag("row.attachment")[1].performScrollTo().performClick()
        try {
            compose.waitUntil(TIMEOUT_MS) { readStub("/debug/fetched").contains("1") }
        } catch (e: Throwable) {
            // This failed once in fourteen and passed three times
            // isolated, so the next occurrence has to arrive as an
            // answer rather than another re-run: whether the tap was
            // lost (nothing fetched, no error) or the fetch failed
            // (an error on screen) are different bugs.
            // Counts, not a truncated tree: the tree was cut off at
            // 2000 characters and its silence about the attachment rows
            // proved nothing, which is the failure mode this diagnostic
            // was added to avoid.
            throw AssertionError(
                "no attachment was fetched. /debug/fetched=" + readStub("/debug/fetched") +
                    "; attachment rows now " +
                    compose.onAllNodesWithTag("row.attachment").fetchSemanticsNodes().size +
                    ", message bodies " +
                    compose.onAllNodesWithTag("body.web").fetchSemanticsNodes().size,
                e,
            )
        }
        assertTrue(
            "index 0 was fetched, not the row that was tapped",
            !readStub("/debug/fetched").contains("0"),
        )
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

    /** And the composer goes back to whatever opened it. */
    @Test
    fun back_cancels_the_composer() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")

        pressBack()
        waitForTag("list.conversations", "back did not leave the composer")
    }

    /** And it collapses the search rather than leaving. */
    @Test
    fun back_collapses_the_search() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.search").performClick()
        waitForTag("search.field", "the search never opened")

        pressBack()
        waitForTag("list.conversations", "back did not return to the inbox")
    }

    private fun pressBack() {
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
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
     * Leaving a mailing list, asserted at the wire.
     *
     * The request names a **message**, never a URL: the advertised URLs
     * identify the subscriber, so a client that posted to one itself
     * would hand the sender the reader's address and network — and the
     * stub answers 400 to any body carrying a URL, so this fails rather
     * than passes if that ever changes.
     */
    @Test
    fun unsubscribing_names_the_message_not_a_url() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.NP").performClick()

        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithText("This week in systems design").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithText("This week in systems design").performClick()
        waitForTag("list.messages", "the newsletter never opened")

        // The one-click offer is the second message; the first only
        // advertises a page, and tapping that leaves for a browser —
        // which is the correct behaviour for that offer and useless here.
        compose.waitUntil(TIMEOUT_MS) {
            compose.onAllNodesWithTag("button.unsubscribe").fetchSemanticsNodes().size == 2
        }
        compose.onAllNodesWithTag("button.unsubscribe")[1].performScrollTo().performClick()

        waitForTag("unsubscribed", "the button never resolved to a result")

        val asked = readStub("/debug/unsubscribed")
        assertTrue("the request did not name thread t3: $asked", asked.contains("\"t3\""))
        assertTrue("the request did not name uid 8: $asked", asked.contains("8"))
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

    /** And back leaves the selection rather than the app. */
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
     * Cc and Bcc reach the wire, and a suggestion lands in its own line.
     *
     * The two are one test because the failure they share is the same
     * one: a suggestion that could fill the wrong recipient line is how
     * a message goes to somebody who was never meant to see it. The Cc
     * here is completed from the contact list, and the assertion is that
     * it arrived as **cc** and not as another **to**.
     */
    @Test
    fun cc_and_bcc_travel_as_themselves() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")

        compose.onNodeWithTag("field.to").performTextInput("someone@example.com")
        compose.onNodeWithTag("button.ccBcc").performClick()
        waitForTag("field.cc", "Cc never appeared")

        compose.onNodeWithTag("field.cc").performTextInput("ali")
        waitForTag("suggestion.contact", "no contact was suggested")
        compose.onAllNodesWithTag("suggestion.contact").onFirst().performClick()

        compose.onNodeWithTag("field.bcc").performTextInput("bob@example.com")
        compose.onNodeWithTag("field.subject").performTextInput("Three lines")
        compose.onNodeWithTag("field.body").performTextInput("body")
        compose.onNodeWithTag("button.send").performClick()

        compose.waitUntil(TIMEOUT_MS) { readStub("/debug/sent").contains("Three lines") }
        val sent = readStub("/debug/sent")
        assertTrue("the Cc did not travel as a Cc: $sent", sent.contains("\"cc\": [\"alice@example.com\"]"))
        assertTrue("the Bcc did not travel as a Bcc: $sent", sent.contains("\"bcc\": [\"bob@example.com\"]"))
    }

    private fun readStub(path: String): String {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL") ?: DEFAULT_STUB
        return java.net.URL(stub + path).openStream().bufferedReader().use { it.readText() }
    }

    private companion object {
        /** The emulator's route to the host, where the stub runs. */
        const val DEFAULT_STUB = "http://10.0.2.2:6039"
        const val TIMEOUT_MS = 15_000L
    }
}
