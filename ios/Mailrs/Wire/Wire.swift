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
