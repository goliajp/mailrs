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
 * Writing a message, and everything that starts one.
 *
 * A reply, a draft, an attachment, a `mailto:` link, a shared file.
 * Split out of `MailFlowTest` when it reached 1,294 lines against this
 * repo's 500-line limit.
 */
@RunWith(AndroidJUnit4::class)
class ComposeFlowTest : MailrsUiTest() {

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

        // Under the body, so the same rule as every other control down
        // there: the WebView finds its height after its content loads
        // and everything below slides. This failed as "the composer
        // never opened", which is what a lost tap looks like.
        tapWhenSteady("button.reply", 0)
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

    /**
     * Leaving the composer keeps what was written, and editing it again
     * updates the same draft.
     *
     * Both halves matter and the second is invisible on screen: a save
     * that dropped the id would leave a new draft behind on every edit,
     * and the list would look plausible while filling up with copies of
     * one message. `/debug/draft-posts` records the id each POST carried
     * — null the first time, the allocated id every time after.
     */
    @Test
    fun leaving_the_composer_saves_a_draft_and_editing_reuses_it() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.subject", "the composer never opened")
        compose.onNodeWithTag("field.subject").performTextInput("Half a thought")

        pressBack()
        waitForTag("list.conversations", "back did not leave the composer")
        compose.waitUntil(TIMEOUT_MS) { readStub("/debug/draft-posts").contains("null") }

        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Drafts").performClick()
        waitForTag("list.drafts", "the draft was not listed")
        compose.onNodeWithText("Half a thought").assertIsDisplayed()

        // Reopen, add to it, and leave again.
        compose.onNodeWithText("Half a thought").performClick()
        waitForTag("field.subject", "the draft did not reopen")
        compose.onNodeWithTag("field.body").performTextInput("and the rest")
        pressBack()

        compose.waitUntil(TIMEOUT_MS) {
            // The second post carries the id the first one was given.
            readStub("/debug/draft-posts").contains("1")
        }
        val posts = readStub("/debug/draft-posts")
        assertTrue("the second save did not name the draft: $posts", posts.contains("[null, 1]"))
    }

    /**
     * A file goes out with the message, by name and by content type.
     *
     * The system picker is skipped — it runs in another process and is
     * the platform's. What this covers is everything after it: the name
     * and size read from the content resolver rather than from the
     * URI's opaque last segment, the body streamed instead of read into
     * memory, and the multipart field names the handler reads. The stub
     * records what arrived, so the assertion is on the file the server
     * got and not on the chip that was drawn.
     */
    @Test
    fun an_attached_file_arrives_with_the_message() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")

        compose.activityRule.scenario.onActivity { activity ->
            val dir = java.io.File(activity.cacheDir, "attachments/test")
            dir.mkdirs()
            val file = java.io.File(dir, "notes.txt")
            file.writeText("two lines\nof it\n")
            activity.attachForTest(
                androidx.core.content.FileProvider.getUriForFile(
                    activity,
                    activity.packageName + ".files",
                    file,
                ),
            )
        }
        waitForTag("row.draftAttachment", "the file was never taken on")
        compose.onNodeWithText("notes.txt").assertIsDisplayed()

        compose.onNodeWithTag("field.to").performTextInput("someone@example.com")
        compose.onNodeWithTag("field.subject").performTextInput("With a file")
        compose.onNodeWithTag("button.send").performClick()

        compose.waitUntil(TIMEOUT_MS) { readStub("/debug/sent").contains("With a file") }
        val sent = readStub("/debug/sent")
        assertTrue("the file did not arrive: $sent", sent.contains("notes.txt"))
        // 16 bytes of "two lines\nof it\n" — a file that arrived empty
        // would still carry the right name.
        assertTrue("the file arrived empty: $sent", sent.contains("\"bytes\": 16"))
    }

    /**
     * A `mailto:` link opens the composer already addressed.
     *
     * Driven through the activity's real intent path, not by calling
     * the view model: what is worth testing is that the manifest filter
     * and the parsing meet, and a test that skipped the intent would
     * pass with the filter missing.
     */
    @Test
    fun a_mailto_link_opens_an_addressed_composer() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.activityRule.scenario.onActivity { activity ->
            activity.deliverForTest(
                android.content.Intent(
                    android.content.Intent.ACTION_VIEW,
                    android.net.Uri.parse("mailto:a%2Btag@x.test?subject=Hello%20there"),
                ),
            )
        }

        waitForTag("field.to", "the composer never opened")
        compose.onNodeWithTag("field.to").assertTextContains("a+tag@x.test")
        compose.onNodeWithTag("field.subject").assertTextContains("Hello there")
    }

    /**
     * A shared file arrives attached, and the text arrives as the body.
     *
     * The share sheet is the other half of being a mail client on this
     * phone: a photo shared to Mailrs has to end up on a message.
     */
    @Test
    fun a_shared_file_arrives_attached() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.activityRule.scenario.onActivity { activity ->
            val dir = java.io.File(activity.cacheDir, "attachments/shared")
            dir.mkdirs()
            val file = java.io.File(dir, "shared.txt")
            file.writeText("from another app")
            val uri = androidx.core.content.FileProvider.getUriForFile(
                activity,
                activity.packageName + ".files",
                file,
            )
            activity.deliverForTest(
                android.content.Intent(android.content.Intent.ACTION_SEND).apply {
                    type = "text/plain"
                    putExtra(android.content.Intent.EXTRA_SUBJECT, "Look at this")
                    putExtra(android.content.Intent.EXTRA_STREAM, uri)
                },
            )
        }

        waitForTag("row.draftAttachment", "the shared file was never taken on")
        compose.onNodeWithText("shared.txt").assertIsDisplayed()
        compose.onNodeWithTag("field.subject").assertTextContains("Look at this")
    }

    /**
     * The manifest actually offers to handle these.
     *
     * The behaviour tests deliver an intent straight to the activity,
     * which would pass just as well with no `<intent-filter>` at all —
     * and then nothing on the phone would ever send one. This asks the
     * package manager the question a link tap asks.
     */
    @Test
    fun the_manifest_offers_to_handle_mail_intents() {
        val pm = InstrumentationRegistry.getInstrumentation().targetContext.packageManager
        val mailto = android.content.Intent(
            android.content.Intent.ACTION_VIEW,
            android.net.Uri.parse("mailto:a@x.test"),
        )
        val share = android.content.Intent(android.content.Intent.ACTION_SEND).setType("text/plain")
        for ((what, intent) in listOf("mailto" to mailto, "share" to share)) {
            val ours = pm.queryIntentActivities(intent, 0).any {
                it.activityInfo.packageName == "jp.golia.mailrs"
            }
            assertTrue("nothing in this app answers a $what intent", ours)
        }
    }

    @Test
    fun the_search_shortcut_opens_the_search() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")

        compose.activityRule.scenario.onActivity { activity ->
            activity.deliverForTest(android.content.Intent("jp.golia.mailrs.SEARCH"))
        }
        waitForTag("search.field", "the shortcut did not open the search")
    }

    @Test
    fun back_cancels_the_composer() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")

        pressBack()
        waitForTag("list.conversations", "back did not leave the composer")
    }

    /**
     * Sending publishes the recipient to the system share sheet.
     *
     * The row at the top of Android's share sheet is built from an
     * app's dynamic shortcuts, so "share this photo to Alice" only
     * works if somebody was written to first. Published from people
     * actually written to, never from the address book — a sheet
     * offering everyone this account has *received* from would put a
     * mailing list one tap away from a photo.
     */
    @Test
    fun sending_puts_the_recipient_in_the_share_sheet() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")

        compose.onNodeWithTag("field.to").performTextInput("alice@example.com")
        compose.onNodeWithTag("field.subject").performTextInput("Shortcut please")
        compose.onNodeWithTag("field.body").performTextInput("body")
        compose.onNodeWithTag("button.send").performClick()
        compose.waitUntil(TIMEOUT_MS) { readStub("/debug/sent").contains("Shortcut please") }

        val context = InstrumentationRegistry.getInstrumentation().targetContext
        compose.waitUntil(TIMEOUT_MS) {
            androidx.core.content.pm.ShortcutManagerCompat.getDynamicShortcuts(context)
                .any { it.id == "recipient:alice@example.com" }
        }
    }

    /**
     * A message sent from the phone is signed by the account.
     *
     * The web keeps its signature in `localStorage`, so it belongs to a
     * browser rather than a person; the server has had a per-user store
     * the whole time and this client reads it, which makes the
     * signature follow the account. The stub offers two and marks the
     * second default — picking the first would sign work mail "Sent
     * from a phone" forever.
     */
    @Test
    fun a_sent_message_carries_the_accounts_signature() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")

        compose.onNodeWithTag("field.to").performTextInput("someone@example.com")
        compose.onNodeWithTag("field.subject").performTextInput("Signed please")
        compose.onNodeWithTag("field.body").performTextInput("Short note.")
        compose.onNodeWithTag("button.send").performClick()

        compose.waitUntil(TIMEOUT_MS) { readStub("/debug/sent").contains("Signed please") }
        val sent = readStub("/debug/sent")
        // The separator carries its trailing space; JSON escapes the
        // newlines, which is why this looks the way it does.
        assertTrue("the message went out unsigned: $sent", sent.contains("-- \\nLi Hao"))
        assertTrue("it signed with the wrong one: $sent", !sent.contains("Sent from a phone"))
    }

    /**
     * Forwarding passes the message on, attachments and all.
     *
     * Three things that are each invisible when wrong: the subject is
     * "Fwd:" and not "Re:", the recipients start empty because it is
     * going to somebody new, and `forward_attachments_from` carries the
     * original's files — the server re-extracts them, so a phone can
     * forward what it has never downloaded. A forward that dropped that
     * field would look identical on screen and arrive with nothing
     * attached.
     *
     * It is deliberately not a reply: `in_reply_to` would thread it
     * into a conversation the new recipient has never been in.
     */
    @Test
    fun forwarding_carries_the_subject_and_the_attachments() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onAllNodesWithTag("row.conversation").onFirst().performClick()
        waitForTag("list.messages", "the thread never opened")

        tapWhenSteady("button.forward", 0)
        waitForTag("field.to", "the composer never opened")
        compose.onNodeWithTag("field.subject").assertTextContains("Fwd:", substring = true)
        compose.onNodeWithTag("field.to").assertTextContains("", substring = true)

        compose.onNodeWithTag("field.to").performTextInput("elsewhere@example.com")
        compose.onNodeWithTag("button.send").performClick()
        compose.waitUntil(TIMEOUT_MS) { readStub("/debug/sent").contains("Fwd:") }

        val sent = readStub("/debug/sent")
        assertTrue("the original's attachments were not carried: $sent",
            sent.contains("\"forward_attachments_from\": 1"))
        assertTrue("a forward should not thread into the original: $sent",
            !sent.contains("\"in_reply_to\": \"<m1@x>\""))
    }
}
