import Foundation

/// The shapes the mailrs REST API actually sends.
///
/// Written against the Rust handlers, not against `openapi.json` — that
/// file has drifted before, and the web client shipped nine schemas
/// disagreeing with the backend because they were written from it
/// (`.claude/rules/frontend/wire-schema-verification.md`). Each type
/// below names the handler it mirrors so the next person can check.
enum Wire {
    /// Backend: `crates/webapi/src/handlers/auth/session.rs` — `LoginRequest`.
    struct LoginRequest: Encodable {
        let address: String
        let password: String
        let totpCode: String?

        enum CodingKeys: String, CodingKey {
            case address
            case password
            case totpCode = "totp_code"
        }
    }

    /// Backend: `crates/webapi/src/handlers/auth/session.rs` — `LoginResponse`.
    ///
    /// The handler also has a `{ requires_totp: true }` short-circuit
    /// before a session is issued, which is why the decode is attempted
    /// as `TotpChallenge` first in `MailrsClient.logIn`.
    struct LoginResponse: Decodable {
        let address: String
        let displayName: String
        let permissions: [String]
        let token: String

        enum CodingKeys: String, CodingKey {
            case address
            case displayName = "display_name"
            case permissions
            case token
        }
    }

    struct TotpChallenge: Decodable {
        let requiresTotp: Bool

        enum CodingKeys: String, CodingKey {
            case requiresTotp = "requires_totp"
        }
    }

    /// Backend: `crates/webapi/src/handlers/conversation_body.rs` —
    /// built inline as `serde_json::json!` per attachment part.
    ///
    /// There is no `index` on the wire. The index a URL needs is the
    /// position in the array, which is how `get_attachment` looks it up
    /// (`attachments.get(index)`), so callers must count rather than read
    /// a field. The web client's schema declares an `index` defaulting to
    /// 0 that the server never sends; it happens to be harmless there
    /// only because the UI passes the array position instead.
    struct Attachment: Decodable, Sendable {
        let filename: String
        let contentType: String
        let size: Int
        /// Present only on `multipart/related` inline images — the parts
        /// an HTML body references with `<img src="cid:…">`.
        let contentId: String?

        enum CodingKeys: String, CodingKey {
            case filename
            case contentType = "content_type"
            case size
            case contentId = "content_id"
        }
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

        enum CodingKeys: String, CodingKey {
            case to
            case cc
            case subject
            case body
            case inReplyTo = "in_reply_to"
            case replyToThreadId = "reply_to_thread_id"
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

    /// Backend: `crates/webapi/src/handlers/conversation_body.rs` —
    /// `ThreadMessageResponse`.
    ///
    /// `GET /api/conversations/{thread_id}` returns a bare array of
    /// these, like the list endpoint. Most of the struct is analysis
    /// output this client does not read yet; the fields below are the
    /// ones it does, and `Decodable` ignores the rest.
    struct Message: Decodable, Identifiable, Sendable {
        let uid: UInt32
        let sender: String
        /// `"verified"` (DMARC pass), `"suspicious"` (an auth method
        /// failed — likely spoofed), `"unverified"`, or `""` for mail
        /// that predates the signal. Cryptographic, not a model.
        let senderTrust: String
        let recipients: String
        let subject: String
        let internalDate: Int64
        let messageId: String
        let textBody: String?
        let htmlBody: String?
        let attachments: [Attachment]

        var id: UInt32 { uid }

        enum CodingKeys: String, CodingKey {
            case uid
            case sender
            case senderTrust = "sender_trust"
            case recipients
            case subject
            case internalDate = "internal_date"
            case messageId = "message_id"
            case textBody = "text_body"
            case htmlBody = "html_body"
            case attachments
        }
    }

    /// Backend: `crates/webapi/src/handlers/conversations.rs` —
    /// `ConversationResponse`.
    ///
    /// `GET /api/conversations` returns a **bare array** of these. Not an
    /// envelope: there is no `items` / `has_more` / `next_cursor` wrapper,
    /// however much the shape of every other list endpoint suggests one.
    struct Conversation: Decodable, Identifiable, Sendable {
        let threadId: String
        let subject: String
        let participants: [String]
        let messageCount: Int
        let unreadCount: Int
        let lastDate: Int64
        let category: String
        let flagged: Bool
        let snippet: String
        let pinned: Bool
        let archived: Bool
        let importanceLevel: String
        let importanceScore: Double
        let requiresAction: Bool
        let receivedCount: Int
        let sentCount: Int

        var id: String { threadId }

        enum CodingKeys: String, CodingKey {
            case threadId = "thread_id"
            case subject
            case participants
            case messageCount = "message_count"
            case unreadCount = "unread_count"
            case lastDate = "last_date"
            case category
            case flagged
            case snippet
            case pinned
            case archived
            case importanceLevel = "importance_level"
            case importanceScore = "importance_score"
            case requiresAction = "requires_action"
            case receivedCount = "received_count"
            case sentCount = "sent_count"
        }
    }
}
