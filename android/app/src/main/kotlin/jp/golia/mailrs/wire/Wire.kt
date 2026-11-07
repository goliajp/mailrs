package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.Serializable

/**
 * What the server sends, named as the server names it.
 *
 * **Written against the handlers, not against another client.**
 * The first draft of this file — copied from the iOS Swift types —
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

    /** Backend: `crates/webapi/src/handlers/auth/session.rs` — `LoginRequest`. */
    @Serializable
    data class LoginRequest(
        val address: String,
        val password: String,
        /** Absent rather than empty: the server reads an empty code as a wrong one. */
        @SerialName("totp_code") val totpCode: String? = null,
    )

    /** The 200 the handler sends instead of a token while a TOTP code is missing. */
    @Serializable
    data class TotpChallenge(@SerialName("requires_totp") val requiresTotp: Boolean)

    @Serializable
    data class LoginResponse(val token: String)

    /** One row of `GET /api/conversations` — a bare JSON array. */
    @Serializable
    data class Conversation(
        /**
         * Which connected mailbox this arrived at, empty for this
         * server's own. It decides which address a reply leaves by.
         *
         * Defaulted rather than required: a row stored before
         * connected mailboxes existed carries none, and its absence
         * reads as ours, which is what it is.
         */
        @SerialName("account_id") val accountId: String = "",
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
        /** Unix seconds it comes back to the inbox; absent or past when not snoozed. */
        @SerialName("snoozed_until") val snoozedUntil: Long? = null,
        /** When it went to the Trash, 0 when it is not there. */
        @SerialName("trashed_at") val trashedAt: Long = 0,
        val labels: List<String> = emptyList(),
        /**
         * Where the page after this row starts, handed back as `cursor=`.
         * The server's to read, not ours. Null on a row from a server
         * that does not stamp one.
         */
        val cursor: String? = null,
        /** The team member who answers it, empty for nobody and for a non-team conversation. */
        val assignee: String = "",
        /** The sectioned inbox's section (`reply`, `awaiting`, `new`, `feed`, `other`); empty when undecided. */
        val section: String = "",
        /** The team conversation this is the reader's copy of, `gid:conv`; empty when it is not one. */
        @SerialName("shared_conv") val sharedConv: String = "",
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
        val unsubscribe: Unsubscribe? = null,
        /**
         * iTIP method of the message's `text/calendar` part, upper-case
         * — REQUEST / REPLY / CANCEL / PUBLISH / COUNTER — or empty for
         * mail with no calendar part, which is nearly all of it.
         *
         * Carried on every message of the thread, like [senderTrust]
         * above it, so the timeline can mark an invitation without
         * fetching one. The event itself is a separate request, made
         * only when the card opens.
         */
        @SerialName("invite_method") val inviteMethod: String = "",
        // absent from servers older than the fields
        @SerialName("sender_checks") val senderChecks: List<WireVerdict.Reason> = emptyList(),
        @SerialName("link_warnings") val linkWarnings: List<WireVerdict.LinkWarning> = emptyList(),
    )

    /** The single-message read, of which this client wants the invitation. */
    @Serializable
    data class MessageDetail(
        @SerialName("invite_method") val inviteMethod: String = "",
        @SerialName("invite_payload") val invite: WireInvite? = null,
        /** `ACCEPTED` / `TENTATIVE` / `DECLINED`, or null if unanswered. */
        @SerialName("rsvp_status") val rsvpStatus: String? = null,
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
     * What a message advertises as the way off its list.
     *
     * `one_click` means the sender accepts an RFC 8058 POST, which the
     * **server** performs. The URLs are sent so the client can offer a
     * link when one-click is not on the table, but a client must not
     * post to them itself: they identify the subscriber, and fetching
     * one from a phone hands the sender the reader's address and
     * network.
     *
     * Source: `crates/webapi/src/handlers/conversation_body.rs` —
     * `UnsubscribeWire`. Both vectors are `skip_serializing_if` empty,
     * so absent means none.
     */
    @Serializable
    data class Unsubscribe(
        @SerialName("one_click") val oneClick: Boolean = false,
        val http: List<String> = emptyList(),
        val mailto: List<String> = emptyList(),
    )

    /**
     * `POST /api/mail/unsubscribe` — `{thread_id, uid}`.
     *
     * The message's identity, not a URL from the client. The server
     * takes the one-click URL out of the message's own header, so a
     * client that sent a URL would be choosing where the POST goes.
     */
    @Serializable
    data class UnsubscribeRequest(
        @SerialName("thread_id") val threadId: String,
        val uid: Int,
    )

    @Serializable
    data class UnsubscribeResult(
        val ok: Boolean,
        val status: Int? = null,
        val message: String? = null,
    )

    /**
     * One of the account's signatures.
     *
     * Source: `crates/core-api/src/method/admin/userdata.rs` —
     * `SignatureWire`. `list_signatures` answers a bare array, not
     * `{items: [...]}` like the admin lists.
     */
    @Serializable
    data class Signature(
        val id: Long,
        val name: String = "",
        val html: String = "",
        @SerialName("text_content") val textContent: String = "",
        @SerialName("is_default") val isDefault: Boolean = false,
        @SerialName("created_at") val createdAt: String = "",
    )

    /**
     * What `POST /api/conversations/batch` answers.
     *
     * **200 is not success.** The route applies each verb in turn and
     * reports how many did not go through — and which — in the body. A
     * client that read only the status code would take a partial
     * failure for a clean one and leave rows off the screen that are
     * still in the mailbox.
     *
     * Source: `crates/webapi/src/handlers/conversation_verbs.rs` —
     * `BatchResponse`.
     */
    @Serializable
    data class BatchResult(
        val success: Boolean = true,
        val failed: Int = 0,
        val processed: Int = 0,
        val message: String? = null,
        @SerialName("failed_thread_ids") val failedThreadIds: List<String> = emptyList(),
    )

    /** `GET /api/conversations/unseen-count`. */
    @Serializable
    data class UnseenCount(val count: Int = 0)

    /** `POST /api/conversations/batch` — `{action, thread_ids}`. */
    @Serializable
    data class BatchRequest(
        val action: String,
        @SerialName("thread_ids") val threadIds: List<String>,
        /** The label `label` and `unlabel` put on or take off. */
        @SerialName("label_id") val labelId: String? = null,
    )

    /**
     * `GET /api/mail/sent` — the sent axis, a bare array.
     *
     * What the maildir sweep has filed. Half of the Send list; the
     * other half is [Send], which knows whether it arrived.
     */
    @Serializable
    data class SentMessage(
        val uid: Int,
        @SerialName("message_id") val messageId: String,
        @SerialName("thread_id") val threadId: String,
        val to: String,
        val subject: String,
        @SerialName("internal_date") val internalDate: Long,
    )

    /** `GET /api/mail/sends` — the delivery projection. */
    @Serializable
    data class Send(
        @SerialName("send_id") val sendId: String,
        @SerialName("thread_id") val threadId: String,
        val subject: String,
        val to: List<String> = emptyList(),
        @SerialName("created_at") val createdAt: Long,
        val status: String,
        @SerialName("resent_from") val resentFrom: String? = null,
        /**
         * Whether the server still holds the bytes to send again.
         *
         * Its judgement, not one this side can make: it reads an empty
         * envelope reference as "the bytes are not on disk", and a
         * button offered against that answers 409 after the tap.
         */
        @SerialName("can_resend") val canResend: Boolean = false,
    )

    /** `GET /api/scheduled` — the caller's own future-dated sends. */
    @Serializable
    data class ScheduledListResponse(val items: List<ScheduledSend> = emptyList())

    @Serializable
    data class ScheduledSend(
        val id: String,
        @SerialName("scheduled_at") val scheduledAt: Long,
        val recipient: String,
        val subject: String,
    )

    /**
     * `POST /api/scheduled/{id}/reschedule`. Epoch seconds and in the
     * future — the handler answers 400 for a time already passed.
     */
    @Serializable
    data class RescheduleRequest(@SerialName("scheduled_at") val scheduledAt: Long)

    /** `GET /api/mail/sends/{id}/redraft` — a sent message, to edit. */
    @Serializable
    data class Redraft(
        @SerialName("redraft_of") val redraftOf: String,
        val to: List<String> = emptyList(),
        val cc: List<String> = emptyList(),
        val bcc: List<String> = emptyList(),
        val subject: String = "",
        val body: String = "",
        @SerialName("in_reply_to") val inReplyTo: String? = null,
        /** Described, not transferred: the bytes stay on the server. */
        val attachments: List<RedraftAttachment> = emptyList(),
    )

    @Serializable
    data class RedraftAttachment(
        val index: Int,
        val filename: String,
        @SerialName("content_type") val contentType: String = "",
        val size: Int = 0,
    )

    /**
     * `GET /api/admin/dmarc/sources` — who has been sending as these
     * domains, rolled up per IP over a window.
     */
    @Serializable
    data class DmarcSourceList(
        val items: List<DmarcSource> = emptyList(),
        val total: Long = 0,
        val passing: Long = 0,
    )

    @Serializable
    data class DmarcSource(
        @SerialName("source_ip") val sourceIp: String,
        val total: Long = 0,
        val passing: Long = 0,
        /** The policy domains this source sent as. */
        val domains: List<String> = emptyList(),
    )
}
