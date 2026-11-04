package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A team conversation, read only: its notes, and who is on it. The same
 * rules as the iPhone's `Wire/TeamWire.swift`.
 * Backend: `crates/webapi/src/handlers/shared_inbox/` — `Overview`, `notes::Note`.
 */
object TeamWire {
    /** `gid` and `conv` from a row's `shared_conv` ("1:7"). */
    data class Conv(val gid: Int, val conv: Int) {
        val path: String get() = "/api/shared/$gid/$conv"

        companion object {
            /** Null when it is not a team copy or the value is not two numbers. */
            fun of(sharedConv: String): Conv? {
                val parts = sharedConv.split(":")
                if (parts.size != 2) return null
                val gid = parts[0].toIntOrNull() ?: return null
                val conv = parts[1].toIntOrNull() ?: return null
                return Conv(gid, conv)
            }
        }
    }

    @Serializable
    data class Note(
        val id: Int,
        val at: Long,
        val by: String,
        @SerialName("edited_at") val editedAt: Long? = null,
        val text: String,
    )

    @Serializable
    data class DraftPointer(val author: String)

    @Serializable
    data class Overview(val assignee: String? = null, val draft: DraftPointer? = null)

    /** What a reply says about who is on the conversation. */
    sealed interface ReplyHint {
        data class Writing(val name: String) : ReplyHint
        data class Assigned(val name: String) : ReplyHint
    }

    /**
     * Someone else writing a reply first, then someone else assigned; null
     * when it is the reader's own or nobody's.
     */
    fun replyHint(overview: Overview, me: String): ReplyHint? {
        val mine = me.lowercase()
        val author = overview.draft?.author.orEmpty()
        if (author.isNotEmpty() && author.lowercase() != mine) return ReplyHint.Writing(localPart(author))
        val assignee = overview.assignee.orEmpty()
        if (assignee.isNotEmpty() && assignee.lowercase() != mine) return ReplyHint.Assigned(localPart(assignee))
        return null
    }

    /** `bob` for `bob@golia.jp`, as the row names an assignee. */
    fun localPart(address: String): String {
        val at = address.indexOf('@')
        if (at <= 0) return address
        return address.substring(0, at)
    }
}

suspend fun MailrsClient.teamNotes(c: TeamWire.Conv): MailrsClient.Outcome<List<TeamWire.Note>> =
    decode(get(c.path + "/notes"), TeamWire.Note.serializer())

suspend fun MailrsClient.teamOverview(c: TeamWire.Conv): MailrsClient.Outcome<TeamWire.Overview> =
    one(get(c.path), TeamWire.Overview.serializer())
