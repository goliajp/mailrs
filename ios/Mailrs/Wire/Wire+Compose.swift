import Foundation

/// Writing mail: drafts, and the request that becomes a message.
///
/// Split out of `Wire.swift` when it passed the 500-line limit this
/// repository holds every language to. Same namespace, same shapes —
/// only the file they live in changed.
extension Wire {

    /// Backend: `crates/core-api/src/method/admin/userdata.rs` —
    /// `DraftWire`, served by `prefs.rs::list_drafts`.
    ///
    /// `to` is a **String** here, not the array `SendRequest` takes: a
    /// draft stores what was typed, and parsing it into addresses is the
    /// send's job. Restoring one therefore puts the text back in the
    /// field exactly as it was left.
    struct Draft: Decodable, Identifiable, Sendable {
        let id: Int64
        let to: String
        let cc: String
        let bcc: String
        let subject: String
        let body: String
        let replyToThreadId: String?
        let createdAt: Int64
        let updatedAt: Int64

        enum CodingKeys: String, CodingKey {
            case id
            case to
            case cc
            case bcc
            case subject
            case body
            case replyToThreadId = "reply_to_thread_id"
            case createdAt = "created_at"
            case updatedAt = "updated_at"
        }
    }


    /// Backend: `SaveDraftRequest`. An `id` upserts that draft in place;
    /// without one the server allocates a fresh id — which is why a
    /// compose session keeps the id it is given rather than posting
    /// anonymously on every autosave and leaving a trail of drafts.
    struct SaveDraftRequest: Encodable {
        let id: Int64?
        let to: String
        let cc: String
        let bcc: String
        let subject: String
        let body: String
        let replyToThreadId: String?

        enum CodingKeys: String, CodingKey {
            case id
            case to
            case cc
            case bcc
            case subject
            case body
            case replyToThreadId = "reply_to_thread_id"
        }
    }


    struct SaveDraftResponse: Decodable {
        let id: Int64
    }


    /// Backend: `crates/webapi/src/handlers/compose.rs` — `SendRequest`,
    /// posted to `/api/mail/send`.
    ///
    /// Every field the handler reads is `#[serde(default)]`, so a missing
    /// one is silently an empty string rather than a 400 — which is why
    /// the threading pair below is sent together rather than trusted to
    /// one or the other.
    struct SendRequest: Encodable {
        let to: [String]
        let cc: [String]
        /// Blind copies. The handler has taken these since the composer
        /// was written; the client simply never sent any, so a field the
        /// server understood had no way to be filled.
        let bcc: [String]
        let subject: String
        let body: String
        /// The Message-ID of the message being replied to.
        let inReplyTo: String?
        /// The conversation the reply lives in.
        ///
        /// Both, always. The handler treats this as a fallback for when
        /// `in_reply_to` is absent, and its comment says why it had to
        /// exist: a client can drop `in_reply_to` and nothing notices —
        /// a reply with an attachment arrived unthreaded on prod on
        /// 2026-07-30 while two without attachments were fine the same
        /// day. Sending both costs a field and removes the failure.
        let replyToThreadId: String?
        /// Forwarding, the backend way: with this set (and no client
        /// attachments) the body carries only the typed text — the
        /// server appends the original body and attachments from the
        /// raw .eml. No threading fields on a forward.
        let forwardMessageId: String?
        /// The uid whose attachments ride along on a forward.
        let forwardAttachmentsFrom: UInt32?

        enum CodingKeys: String, CodingKey {
            case to
            case cc
            case bcc
            case subject
            case body
            case inReplyTo = "in_reply_to"
            case replyToThreadId = "reply_to_thread_id"
            case forwardMessageId = "forward_message_id"
            case forwardAttachmentsFrom = "forward_attachments_from"
        }
    }


    /// Backend: `crates/webapi/src/handlers/compose.rs` — `SendResponse`.
    struct SendResponse: Decodable {
        let messageId: String
        let success: Bool
        let message: String?

        enum CodingKeys: String, CodingKey {
            case messageId = "message_id"
            case success
            case message
        }
    }
}
