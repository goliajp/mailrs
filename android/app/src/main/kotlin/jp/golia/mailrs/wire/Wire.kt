package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * What the server sends, named as the server names it.
 *
 * **Written against the handlers, not against another client.**
 * `.claude/rules/frontend/wire-schema-verification.md` requires it, and
 * the first draft of this file — copied from the iOS Swift types —
 * proved the rule twice in five minutes: it declared `snoozed_until`,
 * which `ConversationResponse` does not have at all, and `body_text` /
 * `body_html`, which are really `text_body` / `html_body`. Neither
 * mistake fails a build; both surface as a field that is quietly always
 * null.
 *
 * Sources, verified 2026-08-17:
 * - `crates/webapi/src/handlers/conversations.rs` — `ConversationResponse`
 *   (17 fields) and `get_thread_messages`
 * - `crates/webapi/src/handlers/conversation_body.rs` — `ThreadMessageResponse`
 * - `crates/webapi/src/router/rest.rs` — `POST /api/auth/login`
 *
 * Unknown fields are ignored, because a client must survive a server
 * that has learned something new. Missing fields are **not** given
 * defaults except where the server may genuinely omit them — a default
 * turns "the server stopped sending this" into a plausible value, which
 * is how `snoozed_until` went unnoticed on the web for months.
 */
object Wire {

    @Serializable
    data class LoginRequest(val username: String, val password: String)

    @Serializable
    data class LoginResponse(val token: String)

    /** One row of `GET /api/conversations` — a bare JSON array. */
    @Serializable
    data class Conversation(
        @SerialName("thread_id") val threadId: String,
        val subject: String,
        val participants: List<String>,
        @SerialName("message_count") val messageCount: Int,
        @SerialName("unread_count") val unreadCount: Int,
        @SerialName("last_date") val lastDate: Long,
        val category: String,
        val flagged: Boolean,
        val snippet: String,
        val pinned: Boolean,
        val archived: Boolean,
        @SerialName("importance_level") val importanceLevel: String,
        @SerialName("importance_score") val importanceScore: Float,
        @SerialName("requires_action") val requiresAction: Boolean,
        @SerialName("received_count") val receivedCount: Int,
        @SerialName("sent_count") val sentCount: Int,
    )

    /**
     * One message of `GET /api/conversations/{thread_id}`.
     *
     * `cc` is `skip_serializing_if = "Option::is_none"` on the server,
     * so it is genuinely absent rather than null when there is none.
     */
    @Serializable
    data class Message(
        val uid: Int,
        val sender: String,
        @SerialName("sender_trust") val senderTrust: String,
        val recipients: String,
        val cc: String? = null,
        val subject: String,
        val flags: Int,
        @SerialName("internal_date") val internalDate: Long,
        @SerialName("message_id") val messageId: String,
        @SerialName("text_body") val textBody: String? = null,
        @SerialName("html_body") val htmlBody: String? = null,
        val category: String,
        @SerialName("risk_score") val riskScore: Int,
        @SerialName("risk_reason") val riskReason: String,
        val attachments: List<Attachment> = emptyList(),
    )

    /**
     * One entry of a message's `attachments`.
     *
     * `content_id` is present only for a part a `multipart/related`
     * body references as `cid:` — an inline image, which is part of the
     * message rather than a file offered alongside it. The server omits
     * the field for ordinary attachments, so its absence is the test.
     *
     * Source: `crates/webapi/src/handlers/conversation_body.rs` — the
     * object is built by hand there, not derived from a struct.
     */
    @Serializable
    data class Attachment(
        val filename: String,
        @SerialName("content_type") val contentType: String,
        val size: Long,
        @SerialName("content_id") val contentId: String? = null,
    )

    /**
     * `POST /api/mail/send`.
     *
     * Every field is `#[serde(default)]` on the server
     * (`crates/webapi/src/handlers/compose.rs`), so omitting one is
     * legal — but they are all sent explicitly here. A reply that
     * silently drops its `in_reply_to` still arrives, and starts a new
     * thread, which is the kind of wrong that looks like it worked.
     */
    @Serializable
    data class SendRequest(
        val to: List<String>,
        val cc: List<String> = emptyList(),
        val bcc: List<String> = emptyList(),
        val subject: String,
        val body: String,
        @SerialName("in_reply_to") val inReplyTo: String? = null,
    )

    /** `POST /api/conversations/batch` — `{action, thread_ids}`. */
    @Serializable
    data class BatchRequest(
        val action: String,
        @SerialName("thread_ids") val threadIds: List<String>,
    )
}
