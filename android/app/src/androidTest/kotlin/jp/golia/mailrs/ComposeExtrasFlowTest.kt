package jp.golia.mailrs

import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import jp.golia.mailrs.wire.Texts
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What the composer gained to match the web: the From choice, undoing a send,
 * the formatting row, templates, and a resumed draft keeping its time and
 * address. The stub state each test needs is set here and cleared after.
 */
@RunWith(AndroidJUnit4::class)
class ComposeExtrasFlowTest : MailrsUiTest() {

    @After
    fun clearStub() {
        postStub("/debug/reset", "{}")
    }

    @Test
    fun a_message_leaves_by_the_chosen_address() {
        postStub("/debug/send-as", """{"send_as": ["sales@golia.jp"]}""")
        openComposer("someone@example.com")
        waitForTag("field.from", "no From choice with a send-as address")
        compose.onNodeWithTag("field.from").performClick()
        waitForTag("from.sales@golia.jp", "the send-as address was not offered")
        compose.onNodeWithTag("from.sales@golia.jp").performClick()
        compose.onNodeWithTag("button.send").performClick()

        val sent = lastSent()
        assertEquals("the message did not leave by the chosen address", "sales@golia.jp", sent.optString("from"))
        assertEquals("the send did not ask the server to hold it", 10, sent.optInt("undo_seconds"))
    }

    @Test
    fun undo_stops_a_held_send() {
        openComposer("someone@example.com")
        compose.onNodeWithTag("button.send").performClick()
        val undo = hasText(Texts.get(R.string.common_undo))
        waitUntilShown(undo, "no Undo after sending")
        compose.onNode(undo).performClick()
        waitUntilShown(hasText(Texts.get(R.string.compose_send_cancelled)), "Undo did not report the send cancelled")
        val cancels = JSONObject(readStub("/debug/pending-cancels")).getJSONArray("cancels")
        assertEquals("Undo did not ask the server to stop this send", "<stub-reply@golia.jp>", cancels.optString(0))
    }

    @Test
    fun undo_too_late_says_the_message_left() {
        postStub("/debug/send-late", "{}")
        openComposer("someone@example.com")
        compose.onNodeWithTag("button.send").performClick()
        val undo = hasText(Texts.get(R.string.common_undo))
        waitUntilShown(undo, "no Undo after sending")
        compose.onNode(undo).performClick()
        waitUntilShown(hasText(Texts.get(R.string.compose_undo_too_late)), "a refused Undo was reported as working")
    }

    @Test
    fun the_bold_button_writes_markdown() {
        openComposer("someone@example.com")
        compose.onNodeWithTag("field.body").performTextInput("Hello ")
        compose.onNodeWithTag("button.format.bold").performClick()
        compose.onNodeWithTag("field.body").performTextInput("big")
        compose.onNodeWithTag("button.send").performClick()
        val body = lastSent().optString("body")
        assertTrue("bold did not write Markdown marks: $body", body.startsWith("Hello **big**"))
    }

    @Test
    fun a_template_fills_subject_and_body() {
        openComposer("someone@example.com")
        waitForTag("button.templates", "no template menu")
        compose.onNodeWithTag("button.templates").performClick()
        waitForTag("template.1", "the template was not listed")
        compose.onNodeWithTag("template.1").performClick()
        compose.onNodeWithTag("button.send").performClick()
        val sent = lastSent()
        assertEquals("the template's subject was not applied", "Thank you", sent.optString("subject"))
        assertTrue("the template's body was not applied", sent.optString("body").startsWith("Thank you for your message."))
    }

    @Test
    fun a_resumed_draft_keeps_its_time_and_address() {
        postStub("/debug/send-as", """{"send_as": ["sales@golia.jp"]}""")
        val at = System.currentTimeMillis() / 1000 + 86_400
        postStub(
            "/api/mail/drafts",
            """{"to": "later@example.com", "subject": "Timed", "body": "Later", "from": "sales@golia.jp", "scheduled_at": $at}""",
        )
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Drafts").performScrollTo().performClick()
        waitForTag("row.draft", "the draft was not listed")
        compose.onAllNodesWithTag("row.draft").onFirst().performClick()
        waitForTag("text.scheduled", "the resumed draft lost its time")
        compose.onNodeWithTag("button.send").performClick()

        val sent = lastSent()
        assertEquals("Send dropped the draft's time", at, sent.optLong("scheduled_at"))
        assertEquals("Send dropped the draft's address", "sales@golia.jp", sent.optString("from"))
    }

    private fun openComposer(to: String) {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.body", "the composer never opened")
        compose.onNodeWithTag("field.to").performTextInput(to)
    }

    /** The last body the stub was sent, once there is one. */
    private fun lastSent(): JSONObject {
        var sent = JSONObject(readStub("/debug/sent")).getJSONArray("sent")
        val deadline = System.currentTimeMillis() + TIMEOUT_MS
        while (sent.length() == 0 && System.currentTimeMillis() < deadline) {
            Thread.sleep(200)
            sent = JSONObject(readStub("/debug/sent")).getJSONArray("sent")
        }
        assertTrue("the send never reached the stub", sent.length() > 0)
        return sent.getJSONObject(sent.length() - 1)
    }

    private fun waitUntilShown(matcher: androidx.compose.ui.test.SemanticsMatcher, what: String) {
        val shown = runCatching {
            compose.waitUntil(TIMEOUT_MS) { compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }
        }.isSuccess
        assertTrue(what, shown)
    }

    private fun postStub(path: String, json: String) {
        val stub = InstrumentationRegistry.getArguments().getString("mailrsBaseURL") ?: DEFAULT_STUB
        val c = java.net.URL(stub + path).openConnection() as java.net.HttpURLConnection
        c.requestMethod = "POST"
        c.doOutput = true
        c.setRequestProperty("Content-Type", "application/json")
        c.setRequestProperty("Authorization", "Bearer stub-token")
        c.outputStream.use { it.write(json.toByteArray()) }
        c.inputStream.use { it.readBytes() }
    }
}
