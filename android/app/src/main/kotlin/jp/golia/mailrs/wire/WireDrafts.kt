package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A file kept with a draft. The bytes are in the server's upload store;
 * the draft names them by `blob_id`.
 *
 * Source: `crates/core-api/src/method/admin/userdata.rs` — `DraftAttachment`.
 */
@Serializable
data class DraftAttachment(
    @SerialName("blob_id") val blobId: String,
    val name: String,
    @SerialName("content_type") val contentType: String,
    val size: Long,
)

/**
 * One saved draft. `to`, `cc` and `bcc` are the raw lines as typed,
 * not parsed lists — the server stores what the composer had.
 *
 * Source: `crates/core-api/src/method/admin/userdata.rs` — `DraftWire`.
 * `attachments` is absent when the draft has none.
 */
@Serializable
data class ServerDraft(
    val id: Long,
    val to: String = "",
    val cc: String = "",
    val bcc: String = "",
    val subject: String = "",
    val body: String = "",
    @SerialName("reply_to_thread_id") val replyToThreadId: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    @SerialName("updated_at") val updatedAt: Long = 0,
    val from: String = "",
    @SerialName("scheduled_at") val scheduledAt: Long? = null,
    val attachments: List<DraftAttachment> = emptyList(),
)

/**
 * `POST /api/mail/drafts` — an id means update, its absence means create.
 *
 * `attachments` absent keeps the draft's files and `[]` removes them, so
 * the app always sends the list it shows.
 */
@Serializable
data class SaveDraftRequest(
    val id: Long? = null,
    val to: String,
    val cc: String,
    val bcc: String,
    val subject: String,
    val body: String,
    @SerialName("reply_to_thread_id") val replyToThreadId: String? = null,
    val from: String = "",
    @SerialName("scheduled_at") val scheduledAt: Long? = null,
    val attachments: List<DraftAttachment>? = null,
)

@Serializable
data class SaveDraftResponse(val id: Long)

/**
 * Sending a draft whose files the server keeps.
 *
 * The send names the draft (`draft_id`) and the server attaches its files,
 * so they must be the ones the composer shows: the draft is saved first.
 * When that save fails and the draft has files, nothing is sent — the
 * server would attach the files of the last save, or none. With no files
 * of its own the message goes without naming the draft.
 */
object DraftSend {
    /**
     * Drafts with files are listed only to a client that asks: one that
     * does not name the draft when it sends would send it without them.
     */
    const val LIST_PATH = "/api/mail/drafts?with_attachments=true"

    sealed interface Next {
        data class Send(val draftId: Long?) : Next
        data object Stop : Next
    }

    fun savesFirst(storedFiles: Int, serverId: Long?): Boolean = storedFiles > 0 || serverId != null

    fun afterSave(storedFiles: Int, savedId: Long?): Next = when {
        savedId != null -> Next.Send(savedId)
        storedFiles > 0 -> Next.Stop
        else -> Next.Send(null)
    }
}
