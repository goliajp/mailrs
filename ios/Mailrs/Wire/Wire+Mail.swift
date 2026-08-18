import Foundation

/// Reading mail: a thread, its messages, and what was sent.
///
/// Split out of `Wire.swift` when it passed the 500-line limit this
/// repository holds every language to. Same namespace, same shapes —
/// only the file they live in changed.
extension Wire {

    /// Backend: `crates/webapi/src/handlers/conversation_body.rs` —
    /// built inline as `serde_json::json!` per attachment part.
    ///
    /// There is no `index` on the wire. The index a URL needs is the
    /// position in the array, which is how `get_attachment` looks it up
    /// (`attachments.get(index)`), so callers must count rather than read
    /// a field. The web client's schema declares an `index` defaulting to
    /// 0 that the server never sends; it happens to be harmless there
    /// only because the UI passes the array position instead.
    struct Attachment: Codable, Equatable, Sendable {
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


    /// Backend: `crates/webapi/src/handlers/conversation_body.rs` —
    /// `ThreadMessageResponse`.
    ///
    /// `GET /api/conversations/{thread_id}` returns a bare array of
    /// these, like the list endpoint. Most of the struct is analysis
    /// output this client does not read yet; the fields below are the
    /// ones it does, and `Decodable` ignores the rest.
    struct Message: Codable, Equatable, Identifiable, Sendable {
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
        /// Present when the message carries `List-Unsubscribe`. 42.6%
        /// of real mail does.
        let unsubscribe: Unsubscribe?

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
            case unsubscribe
        }
    }


    /// Backend: `crates/webapi/src/handlers/conversation_body.rs` —
    /// `UnsubscribeWire`.
    ///
    /// The URLs are here so a link can be offered when one-click is not
    /// on the table. **This client must never fetch them.** They
    /// identify one subscriber, so a request from the phone tells the
    /// sender the mail was opened, from which address and on whose
    /// network — the same thing `RemoteBlock` exists to prevent in the
    /// body. One-click goes through the server; anything else opens in
    /// Safari, where the reader has decided to be seen.
    struct Unsubscribe: Codable, Equatable, Sendable {
        let oneClick: Bool
        let http: [String]
        let mailto: [String]

        enum CodingKeys: String, CodingKey {
            case oneClick = "one_click"
            case http
            case mailto
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            oneClick = try c.decodeIfPresent(Bool.self, forKey: .oneClick) ?? false
            // Both arrays are omitted when empty, so a message with only
            // a mailto target decodes with no `http` key at all.
            http = try c.decodeIfPresent([String].self, forKey: .http) ?? []
            mailto = try c.decodeIfPresent([String].self, forKey: .mailto) ?? []
        }
    }


    /// Backend: `crates/webapi/src/handlers/conversations.rs` —
    /// `ConversationResponse`.
    ///
    /// `GET /api/conversations` returns a **bare array** of these. Not an
    /// envelope: there is no `items` / `has_more` / `next_cursor` wrapper,
    /// however much the shape of every other list endpoint suggests one.
    struct Conversation: Codable, Equatable, Identifiable, Sendable {
        let threadId: String
        let subject: String
        let participants: [String]
        let messageCount: Int
        /// `var` because an optimistic read toggle writes it before the
        /// server has answered. Everything else is what the server said.
        var unreadCount: Int
        let lastDate: Int64
        let category: String
        /// `var` for the same reason as `unreadCount` — the star toggle.
        var flagged: Bool
        let snippet: String
        /// `var` for the same reason as `flagged` — the pin toggle
        /// writes it before the server has answered.
        var pinned: Bool
        let archived: Bool
        /// Epoch seconds this reader put the thread away until, or 0.
        ///
        /// Absent from every server before v2.55, where the field was
        /// written to the shared thread row and parsed by nothing —
        /// so snoozing did nothing at all. Optional here because a
        /// client may be talking to an older one.
        var snoozedUntil: Int64?
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
            case snoozedUntil = "snoozed_until"
            case importanceLevel = "importance_level"
            case importanceScore = "importance_score"
            case requiresAction = "requires_action"
            case receivedCount = "received_count"
            case sentCount = "sent_count"
        }
    }


    /// Backend: `crates/core-api/src/method/thread.rs` —
    /// `SentMessageSummary`, a bare array from `GET /api/mail/sent`.
    struct SentMessage: Decodable, Sendable {
        let uid: UInt32
        let messageId: String
        let threadId: String
        let to: String
        let subject: String
        let internalDate: Int64

        enum CodingKeys: String, CodingKey {
            case uid
            case messageId = "message_id"
            case threadId = "thread_id"
            case to
            case subject
            case internalDate = "internal_date"
        }
    }


    /// Backend: `crates/webapi/src/handlers/sends/mod.rs` —
    /// `SendResponse`, a bare array from `GET /api/mail/sends`. Status is
    /// one of scheduled / sending / delivered / failed / partial
    /// (`core-sidestate/src/families/send/mod.rs`).
    struct Send: Decodable, Sendable {
        let sendId: String
        let threadId: String
        let subject: String
        let to: [String]
        let createdAt: Int64
        let status: String
        let resentFrom: String?
        /// Whether the server still holds the bytes to send again.
        ///
        /// Its judgement, not one this side can make: it reads an empty
        /// envelope reference as "the bytes are not on disk" and
        /// answers 409, so a button offered against that fails after
        /// the tap.
        let canResend: Bool

        enum CodingKeys: String, CodingKey {
            case sendId = "send_id"
            case threadId = "thread_id"
            case subject
            case to
            case createdAt = "created_at"
            case status
            case resentFrom = "resent_from"
            case canResend = "can_resend"
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            sendId = try c.decode(String.self, forKey: .sendId)
            threadId = try c.decode(String.self, forKey: .threadId)
            subject = try c.decode(String.self, forKey: .subject)
            to = try c.decodeIfPresent([String].self, forKey: .to) ?? []
            createdAt = try c.decode(Int64.self, forKey: .createdAt)
            status = try c.decode(String.self, forKey: .status)
            resentFrom = try c.decodeIfPresent(String.self, forKey: .resentFrom)
            canResend = try c.decodeIfPresent(Bool.self, forKey: .canResend) ?? false
        }
    }
}

extension Wire {
    /// One stored signature. The server keeps several per user and
    /// marks one default; this client edits the default and leaves the
    /// rest alone, because a phone is not where anyone maintains a set
    /// of them.
    struct Signature: Decodable, Identifiable, Sendable {
        let id: Int64
        let name: String
        let textContent: String
        let isDefault: Bool

        enum CodingKeys: String, CodingKey {
            case id
            case name
            case textContent = "text_content"
            case isDefault = "is_default"
        }
    }

    struct SaveSignatureRequest: Encodable {
        let name: String
        let textContent: String
        let isDefault: Bool

        enum CodingKeys: String, CodingKey {
            case name
            case textContent = "text_content"
            case isDefault = "is_default"
        }
    }

    struct SaveSignatureResponse: Decodable, Sendable {
        let id: Int64
    }
}

extension Wire {
    /// Backend: `SnoozeBody { snoozed_until: i64 }`. Epoch **seconds**
    /// and an integer — the web posted an ISO 8601 string here for as
    /// long as scheduling existed and every request 422'd.
    struct SnoozeRequest: Encodable {
        let snoozedUntil: Int64

        enum CodingKeys: String, CodingKey {
            case snoozedUntil = "snoozed_until"
        }
    }
}

extension Wire {
    /// Backend: `crates/webapi/src/handlers/spam_lists.rs` —
    /// `{"entries": [...]}`, and `AddRequest { address }`.
    struct SenderListResponse: Decodable, Sendable {
        let entries: [String]
    }

    struct AddSenderRequest: Encodable {
        let address: String
    }
}

