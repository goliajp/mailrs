package jp.golia.mailrs.wire

import kotlinx.serialization.SerialName
import kotlinx.serialization.json.JsonElement
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
    )

    /**
     * A meeting invitation, as the server read it out of the message.
     *
     * Backend: `crates/webapi/src/handlers/complete.rs` —
     * `get_message_single`'s `invite_payload`.
     */
    @Serializable
    data class Invite(
        val uid: String = "",
        /**
         * Higher on every re-send. Exchange does not send
         * `METHOD:UPDATE` — it re-sends the whole invitation as a
         * `REQUEST` with a higher sequence — so this is what tells an
         * update from a first invitation.
         */
        val sequence: Int = 0,
        val summary: String = "",
        val location: String? = null,
        val organizer: Person? = null,
        val attendees: List<InviteAttendee> = emptyList(),
        val status: String? = null,
        /**
         * **The instant, resolved on the server** against the
         * invitation's own `VTIMEZONE`, RFC 3339.
         *
         * Read this, not the wall-clock. A `TZID` is routinely a
         * Windows name — `Pacific Standard Time`, which says "Standard"
         * while the event is in daylight time — and no client-side
         * parser can evaluate one. Null for an all-day event, which has
         * no instant: a date has no offset, and giving it one moves it
         * a day.
         */
        @SerialName("dtstart_utc") val startsAt: String? = null,
        @SerialName("dtend_utc") val endsAt: String? = null,
        /** The wall-clock and zone the organiser wrote, for the second line. */
        val dtstart: JsonElement? = null,
    )

    @Serializable
    data class Person(val cn: String? = null, val email: String = "")

    @Serializable
    data class InviteAttendee(
        val cn: String? = null,
        val email: String = "",
        /** `NEEDS-ACTION` / `ACCEPTED` / `DECLINED` / `TENTATIVE`. */
        val partstat: String = "NEEDS-ACTION",
    )

    /** The single-message read, of which this client wants the invitation. */
    @Serializable
    data class MessageDetail(
        @SerialName("invite_method") val inviteMethod: String = "",
        @SerialName("invite_payload") val invite: Invite? = null,
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
     * One saved draft. `to`, `cc` and `bcc` are the raw lines as typed,
     * not parsed lists — the server stores what the composer had.
     *
     * Source: `crates/core-api/src/method/admin/userdata.rs` —
     * `DraftWire`.
     */
    @Serializable
    data class Draft(
        val id: Long,
        val to: String = "",
        val cc: String = "",
        val bcc: String = "",
        val subject: String = "",
        val body: String = "",
        @SerialName("reply_to_thread_id") val replyToThreadId: String? = null,
        @SerialName("created_at") val createdAt: Long = 0,
        @SerialName("updated_at") val updatedAt: Long = 0,
    )

    /** `POST /api/mail/drafts` — an id means update, its absence means create. */
    @Serializable
    data class SaveDraftRequest(
        val id: Long? = null,
        val to: String,
        val cc: String,
        val bcc: String,
        val subject: String,
        val body: String,
        @SerialName("reply_to_thread_id") val replyToThreadId: String? = null,
    )

    @Serializable
    data class SaveDraftResponse(val id: Long)

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
        /**
         * When it should leave, in epoch **seconds**, or null for now.
         *
         * An integer, not a formatted time: the handler reads anything
         * it cannot parse as "not scheduling", which is how the web's
         * ISO 8601 string once made every scheduled send go out at
         * once.
         */
        @SerialName("scheduled_at") val scheduledAt: Long? = null,
        /** The send this is a re-edit of; the server carries its bytes. */
        @SerialName("redraft_of") val redraftOf: String? = null,
        /**
         * Which carried attachments to keep, by index.
         *
         * **Absent and empty mean different things** — absent keeps
         * every carried attachment, an empty list keeps none — so this
         * is nullable rather than defaulted, and a re-edit that removed
         * every file must send `[]` and not nothing.
         */
        @SerialName("redraft_keep") val redraftKeep: List<Int>? = null,
        /**
         * The message being forwarded, by uid.
         *
         * The server re-extracts its attachments and carries them,
         * which is the whole reason a forward does not have to
         * download and re-upload what it is passing on — and the
         * reason a phone can forward a message it has never opened.
         */
        @SerialName("forward_attachments_from") val forwardAttachmentsFrom: Int? = null,
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
