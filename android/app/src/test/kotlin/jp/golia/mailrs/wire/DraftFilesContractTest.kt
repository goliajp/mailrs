package jp.golia.mailrs.wire

import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Files kept with a draft: the save and the send bodies are the request
 * fixtures the server's tests read, and a send of a draft with files
 * never goes out without the draft having taken them first.
 */
class DraftFilesContractTest {
    // the fixtures spell out every field, defaults included
    private val spelled = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
        encodeDefaults = true
    }

    // the client's own settings
    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

    private val quote = DraftAttachment(
        blobId = "U3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7",
        name = "quote.pdf",
        contentType = "application/pdf",
        size = 48213,
    )

    @Test
    fun `a draft save with files is the shared fixture`() {
        val req = SaveDraftRequest(
            id = 42,
            to = "nagata@nagatax.tokyo.jp",
            cc = "",
            bcc = "",
            subject = "Quote",
            body = "The quote is attached.",
            attachments = listOf(quote),
        )
        val sent = spelled.encodeToString(SaveDraftRequest.serializer(), req)
        assertEquals(fixture("draft-save-attachments"), Json.parseToJsonElement(sent))
    }

    @Test
    fun `a send of a saved draft is the shared fixture`() {
        val req = SendRequest(
            from = "lihao@golia.jp",
            to = listOf("nagata@nagatax.tokyo.jp"),
            subject = "Quote",
            body = "The quote is attached.",
            undoSeconds = 10,
            htmlBody = "<p>The quote is attached.</p>",
            draftId = 42,
        )
        val sent = spelled.encodeToString(SendRequest.serializer(), req)
        assertEquals(fixture("send-draft"), Json.parseToJsonElement(sent))
    }

    @Test
    fun `no files clears them and no list keeps them`() {
        val base = SaveDraftRequest(to = "", cc = "", bcc = "", subject = "s", body = "")
        val cleared = json.parseToJsonElement(
            json.encodeToString(SaveDraftRequest.serializer(), base.copy(attachments = emptyList())),
        ).jsonObject
        assertEquals(0, cleared["attachments"]?.jsonArray?.size)
        val kept = json.parseToJsonElement(
            json.encodeToString(SaveDraftRequest.serializer(), base),
        ).jsonObject
        assertFalse("attachments" in kept)
    }

    @Test
    fun `a listed draft reads its files, and one without has none`() {
        val with = json.decodeFromString(
            ServerDraft.serializer(),
            """{"id":7,"subject":"Quote","attachments":[{"blob_id":"Uab","name":"quote.pdf","content_type":"application/pdf","size":3}]}""",
        )
        assertEquals(listOf(DraftAttachment("Uab", "quote.pdf", "application/pdf", 3)), with.attachments)
        assertTrue(json.decodeFromString(ServerDraft.serializer(), """{"id":8}""").attachments.isEmpty())
    }

    @Test
    fun `a draft with files is saved before it is sent, and not sent when that fails`() {
        assertTrue(DraftSend.savesFirst(storedFiles = 1, serverId = null))
        assertTrue(DraftSend.savesFirst(storedFiles = 0, serverId = 3))
        assertFalse(DraftSend.savesFirst(storedFiles = 0, serverId = null))

        assertEquals(DraftSend.Next.Send(draftId = 9), DraftSend.afterSave(storedFiles = 1, savedId = 9))
        assertEquals(DraftSend.Next.Stop, DraftSend.afterSave(storedFiles = 1, savedId = null))
        // nothing of the draft's to carry: the message goes without it
        assertEquals(DraftSend.Next.Send(draftId = null), DraftSend.afterSave(storedFiles = 0, savedId = null))
    }

    @Test
    fun `drafts are asked for with their files`() {
        assertEquals("/api/mail/drafts?with_attachments=true", DraftSend.LIST_PATH)
    }

    private fun fixture(name: String) =
        Json.parseToJsonElement(File("../../wire-contract/requests/$name.json").readText())
}
