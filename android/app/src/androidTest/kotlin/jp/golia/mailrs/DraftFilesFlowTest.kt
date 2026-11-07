package jp.golia.mailrs

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import jp.golia.mailrs.wire.Texts
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A file added in the composer is kept with the draft.
 *
 * It goes to the server the moment it is added, every save of the draft
 * names it, a reopened draft lists it, and the send names the draft so
 * the server attaches the file — it is not uploaded a second time. The
 * stub records each upload, each draft's files and each send, so the
 * assertions are on what reached the server.
 */
@RunWith(AndroidJUnit4::class)
class DraftFilesFlowTest : MailrsUiTest() {

    @Test
    fun a_file_survives_leaving_and_goes_with_the_reopened_draft() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")

        attach("quote.txt", "the quote\n")
        compose.waitUntil(TIMEOUT_MS) { uploads().size == 1 }
        compose.onNodeWithTag("field.to").performTextInput("nagata@example.com")
        compose.onNodeWithTag("field.subject").performTextInput("Quote")
        compose.onNodeWithTag("button.cancel").performClick()
        waitForTag("list.conversations", "the composer never closed")

        val blob = uploads().single().jsonObject["blob_id"]!!.jsonPrimitive.content
        compose.waitUntil(TIMEOUT_MS) { draftFiles().values.any { it.toString().contains(blob) } }

        openDrafts()
        compose.onAllNodesWithTag("row.draft").onFirst().performClick()
        waitForTag("row.draftAttachment", "the reopened draft listed no file")
        compose.onNodeWithText("quote.txt").assertIsDisplayed()

        compose.onNodeWithTag("button.send").performClick()
        compose.waitUntil(TIMEOUT_MS) { readStub("/debug/sent").contains("Quote") }
        val sent = Json.parseToJsonElement(readStub("/debug/sent")).jsonObject["sent"]!!.jsonArray
            .single().jsonObject
        val draftId = draftIdOf(sent)
        assertTrue("the send did not name the draft: $sent", draftId != null)
        assertFalse(
            "the file went in the form again: $sent",
            sent["attachments"]?.toString().orEmpty().contains("quote.txt"),
        )
        assertEquals("the file was uploaded a second time", 1, uploads().size)
    }

    @Test
    fun a_kept_file_opens_through_the_draft_file_route() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")
        attach("terms.txt", "terms\n")
        compose.waitUntil(TIMEOUT_MS) { uploads().size == 1 }
        compose.onNodeWithTag("field.subject").performTextInput("Terms")
        compose.onNodeWithTag("button.cancel").performClick()
        waitForTag("list.conversations", "the composer never closed")

        openDrafts()
        compose.onAllNodesWithTag("row.draft").onFirst().performClick()
        waitForTag("row.draftAttachment", "the reopened draft listed no file")
        compose.onNodeWithText("terms.txt").performClick()
        val blob = uploads().single().jsonObject["blob_id"]!!.jsonPrimitive.content
        val fetched = runCatching {
            compose.waitUntil(TIMEOUT_MS) { downloads().contains(blob) }
        }.isSuccess
        assertTrue("opening the file never asked the draft file route", fetched)
    }

    @Test
    fun a_refused_file_says_the_servers_reason() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")

        attach("refuse-huge.bin", "too much")
        val shown = runCatching {
            compose.waitUntil(TIMEOUT_MS) {
                compose.onAllNodes(hasText("25 MiB limit", substring = true)).fetchSemanticsNodes().isNotEmpty()
            }
        }.isSuccess
        assertTrue("the server's reason was not shown", shown)
        assertTrue("a refused file was recorded as kept", uploads().isEmpty())
    }

    @Test
    fun a_draft_that_could_not_take_its_files_is_not_sent() {
        signIn()
        waitForTag("list.conversations", "the inbox never listed")
        compose.onNodeWithTag("button.compose").performClick()
        waitForTag("field.to", "the composer never opened")
        attach("quote.txt", "the quote\n")
        compose.waitUntil(TIMEOUT_MS) { uploads().size == 1 }
        compose.onNodeWithTag("field.to").performTextInput("nagata@example.com")
        compose.onNodeWithTag("field.subject").performTextInput("Not yet")

        postStub("/debug/refuse-draft-save")
        compose.onNodeWithTag("button.send").performClick()

        val said = runCatching {
            compose.waitUntil(TIMEOUT_MS) {
                compose.onAllNodes(hasTestTag("text.sendError") and hasText(Texts.get(R.string.compose_files_not_saved)))
                    .fetchSemanticsNodes().isNotEmpty()
            }
        }.isSuccess
        assertTrue("the composer did not say the files were not saved", said)
        assertFalse("the message went anyway", readStub("/debug/sent").contains("Not yet"))
    }

    private fun attach(name: String, text: String) {
        compose.activityRule.scenario.onActivity { activity ->
            val dir = java.io.File(activity.cacheDir, "attachments/draft-files")
            dir.mkdirs()
            val file = java.io.File(dir, name)
            file.writeText(text)
            activity.attachForTest(
                androidx.core.content.FileProvider.getUriForFile(activity, activity.packageName + ".files", file),
            )
        }
        waitForTag("row.draftAttachment", "the file was never taken on")
    }

    private fun openDrafts() {
        compose.onNodeWithTag("button.folders").performClick()
        waitForTag("drawer.lists", "the drawer never opened")
        compose.onNodeWithTag("drawer.item.Drafts").performScrollTo().performClick()
        waitForTag("list.drafts", "the drafts list never opened")
    }

    private fun files(): JsonObject = Json.parseToJsonElement(readStub("/debug/draft-files")).jsonObject

    private fun uploads() = files()["uploads"]!!.jsonArray

    private fun downloads() = files()["downloads"]!!.jsonArray.map { it.jsonPrimitive.content }

    private fun draftFiles() = files()["drafts"]!!.jsonObject

    // a JSON send carries it as a number, a multipart one as a form field
    private fun draftIdOf(sent: JsonObject): Long? {
        val v = sent["draft_id"]?.jsonPrimitive ?: return null
        return v.longOrNull ?: v.content.toLongOrNull()
    }

    private fun postStub(path: String) {
        val c = java.net.URL(stubBase() + path).openConnection() as java.net.HttpURLConnection
        c.requestMethod = "POST"
        c.inputStream.use { it.readBytes() }
    }
}
