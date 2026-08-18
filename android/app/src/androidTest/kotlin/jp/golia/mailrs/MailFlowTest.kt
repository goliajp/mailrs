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
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.printToString
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performScrollToNode
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
class MailFlowTest : MailrsUiTest() {

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
        compose.activityRule.scenario.onActivity { it.useStubServer("http://127.0.0.1:1") }
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
        tapWhenSteady("row.attachment", 1)
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


    /** And the composer goes back to whatever opened it. */

    /** And it collapses the search rather than leaving. */

    /**
     * Scroll to something under a message body, wait for it to stop
     * moving, then tap it.
     *
     * A body is a `WebView` that finds its height after its content
     * loads, so everything below it slides down some milliseconds after
     * the screen looks finished. A tap dispatched across that shift
     * lands where the row *was*. Both failures this fixes were
     * intermittent in the suite and never reproduced in isolation, which
     * is exactly what a layout race looks like.
     *
     * Steady means the top edge is unchanged across two reads, not that
     * some fixed time has passed — a sleep would be a guess about a
     * machine's speed.
     */


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
        tapWhenSteady("button.unsubscribe", 1)

        waitForTag("unsubscribed", "the button never resolved to a result")

        val asked = readStub("/debug/unsubscribed")
        assertTrue("the request did not name thread t3: $asked", asked.contains("\"t3\""))
        assertTrue("the request did not name uid 8: $asked", asked.contains("8"))
    }



    /**
     * The message as it arrived, headers and all.
     *
     * The Received chain and the authentication results are what an
     * operator reaches for when a message did not do what it should
     * have, and nothing else in this app shows them. The endpoint
     * answers `message/rfc822`, not JSON — a client that decoded it
     * would fail on a message that came back perfectly well — so the
     * assertion is on a header line being on screen.
     */
    @Test
    fun a_message_can_be_read_as_it_arrived() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        tapWhenSteady("button.viewSource", 0)
        waitForTag("text.source", "the source never arrived")
        compose.onNodeWithTag("text.source").assertTextContains("Received:", substring = true)
    }

    /**
     * A capped search says so.
     *
     * Search has no keyset parameter, so unlike the conversation list
     * there is no next page to fetch — fifty hits is a ceiling, and
     * showing them as though they were everything that matched is the
     * same silent truncation the list had. The reader can narrow the
     * term, but only if somebody tells them there is more.
     */
    @Test
    fun a_search_that_hit_the_ceiling_says_so() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.search").performClick()
        compose.onNodeWithTag("search.field").performTextInput("many")
        waitForTag("list.searchResults", "the search never returned")

        compose.onNodeWithTag("list.searchResults").performScrollToNode(hasTestTag("search.capped"))
        compose.onNodeWithTag("search.capped").assertTextContains("Narrow the search", substring = true)
    }

    /**
     * The keyboard's own key signs in.
     *
     * A sign-in form where the last field's Done key does nothing makes
     * the reader dismiss the keyboard to reach a button that was under
     * it — which is the arrangement every other Android form avoids.
     */
    @Test
    fun the_keyboard_can_finish_the_sign_in() {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL") ?: DEFAULT_STUB
        compose.activityRule.scenario.onActivity { it.useStubServer(stub) }

        compose.onNodeWithTag("field.address").performTextInput("me@golia.jp")
        compose.onNodeWithTag("field.password").performTextInput("anything")
        compose.onNodeWithTag("field.password").performImeAction()

        waitForTag("list.conversations", "the keyboard's Done key did not sign in")
    }

    /**
     * The app knows whose mailbox it is showing.
     *
     * `myAddress` was read in two places and written in none, so it was
     * always the empty string: Settings said the account was "—", and
     * reply-all, which excludes yourself by comparing against it,
     * excluded nobody — a reply to everyone arrived addressed back at
     * the person who sent it.
     */
    @Test
    fun the_signed_in_address_is_known_to_the_app() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Settings").performClick()
        waitForTag("admin.Aliases", "settings never opened")
        compose.onNodeWithText("me@golia.jp").assertIsDisplayed()
    }

    /**
     * Reply-all does not address the reply back at me.
     *
     * The rule excludes yourself by comparing each recipient against
     * the signed-in address — which the app did not know, so it
     * excluded nobody and every reply-all arrived in its own sender's
     * inbox. The stub's second message is addressed to me and to Bob,
     * so the answer must reach Bob and the sender and stop there.
     */
    @Test
    fun reply_all_leaves_me_off_the_recipients() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        // The **second** message: the first is addressed only to me,
        // so a reply-all to it cannot show the difference between
        // excluding me and having nobody else to exclude.
        tapWhenSteady("button.replyAll", 1)
        waitForTag("field.body", "the composer never opened")
        compose.onNodeWithTag("field.body").performTextInput("Answering everyone.")
        compose.onNodeWithTag("button.send").performClick()
        waitForTag("list.messages", "the composer did not return to the thread after sending")

        val sent = readStub("/debug/sent")
        assertTrue("the reply-all did not reach the other recipient: $sent",
            sent.contains("bob@example.com"))
        assertTrue("the reply-all was addressed back at me: $sent",
            !sent.substringAfter("\"to\"").substringBefore("]").contains("me@golia.jp"))
    }
}
