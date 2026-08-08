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

        enum CodingKeys: String, CodingKey {
            case sendId = "send_id"
            case threadId = "thread_id"
            case subject
            case to
            case createdAt = "created_at"
            case status
            case resentFrom = "resent_from"
        }
    }

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
            case subject
            case body
            case inReplyTo = "in_reply_to"
            case replyToThreadId = "reply_to_thread_id"
            case forwardMessageId = "forward_message_id"
            case forwardAttachmentsFrom = "forward_attachments_from"
        }
    }

    /// Backend: `crates/core-api/src/method/admin/directory.rs` —
    /// `AliasWire`, served by `crates/webapi/src/handlers/admin_directory.rs`.
    ///
    /// `GET /api/admin/aliases` answers `{"items": [...]}` — an
    /// envelope, unlike `/api/conversations`, which is a bare array.
    /// The two shapes live in one app, so the difference is worth
    /// stating rather than remembering.
    struct Alias: Codable, Equatable, Identifiable, Sendable {
        let id: Int64
        let sourceAddress: String
        let targetAddress: String
        let domain: String
        /// `alias` or `forward` — the server's word, shown as-is.
        let aliasType: String
        let active: Bool
        let createdAt: Int64

        enum CodingKeys: String, CodingKey {
            case id
            case sourceAddress = "source_address"
            case targetAddress = "target_address"
            case domain
            case aliasType = "alias_type"
            case active
            case createdAt = "created_at"
        }
    }

    /// Backend: `crates/core-api/src/method/admin/directory.rs` —
    /// `AccountWire`. Same `{items: […]}` envelope as the alias list.
    struct Account: Codable, Equatable, Identifiable, Sendable {
        let address: String
        let domain: String
        let displayName: String
        let active: Bool
        let createdAt: Int64
        let quotaBytes: Int64
        let recoveryEmail: String?

        var id: String { address }

        enum CodingKeys: String, CodingKey {
            case address
            case domain
            case displayName = "display_name"
            case active
            case createdAt = "created_at"
            case quotaBytes = "quota_bytes"
            case recoveryEmail = "recovery_email"
        }
    }

    struct AccountList: Decodable, Sendable {
        let items: [Account]
    }

    /// Backend: `AddAccountRequest`. The password travels in plaintext
    /// over TLS and the server hashes it with Argon2 — so it is held
    /// only for the length of the request, never cached, never logged,
    /// and never written to a draft.
    struct AddAccountRequest: Encodable, Sendable {
        let address: String
        let displayName: String
        let password: String

        enum CodingKeys: String, CodingKey {
            case address
            case displayName = "display_name"
            case password
        }
    }

    /// Backend: `DomainWire`.
    struct Domain: Codable, Equatable, Identifiable, Sendable {
        let name: String
        let createdAt: Int64

        var id: String { name }

        enum CodingKeys: String, CodingKey {
            case name
            case createdAt = "created_at"
        }
    }

    struct DomainList: Decodable, Sendable {
        let items: [Domain]
    }

    struct AddDomainRequest: Encodable, Sendable {
        let name: String
    }

    /// Backend: `crates/core-api/src/method/admin/directory.rs` —
    /// `EmailGroupWire`. One address that delivers to many people,
    /// where an alias delivers to one.
    struct EmailGroup: Codable, Equatable, Identifiable, Sendable {
        let id: Int64
        let address: String
        let domain: String
        let name: String
        let description: String
        let createdAt: Int64

        enum CodingKeys: String, CodingKey {
            case id
            case address
            case domain
            case name
            case description
            case createdAt = "created_at"
        }
    }

    struct EmailGroupList: Decodable, Sendable {
        let items: [EmailGroup]
    }

    /// Backend: `EmailGroupMembersResponse` — bare addresses under
    /// `members`, not objects. The list endpoints in this area use
    /// `items`; this one does not, which is worth saying rather than
    /// discovering.
    struct EmailGroupMembers: Decodable, Sendable {
        let members: [String]
    }

    struct CreateEmailGroupRequest: Encodable, Sendable {
        let address: String
        let domain: String
        let name: String
        let description: String
    }

    struct EmailGroupMemberRequest: Encodable, Sendable {
        let memberAddress: String

        enum CodingKeys: String, CodingKey {
            case memberAddress = "member_address"
        }
    }

    /// Backend: `crates/webapi/src/handlers/complete.rs` —
    /// `list_admin_queue`, which reads the outbound job blob and adds
    /// the list the job was found in as `status`.
    ///
    /// Everything but the identity is optional because the blob is
    /// written at several stages: a job that has never been attempted
    /// has no error and no retry time, and a client that required them
    /// would decode nothing at exactly the moment the queue is healthy.
    struct QueueJob: Decodable, Identifiable, Sendable {
        let id: Int64
        let sender: String
        let recipient: String
        /// `pending` or `inflight` — where the sender found it.
        let status: String
        let attempts: Int?
        let lastError: String?
        let nextRetry: Int64?
        let scheduledAt: Int64?
        let createdAt: Int64?

        enum CodingKeys: String, CodingKey {
            case id
            case sender
            case recipient
            case status
            case attempts
            case lastError = "last_error"
            case nextRetry = "next_retry"
            case scheduledAt = "scheduled_at"
            case createdAt = "created_at"
        }
    }

    struct QueueList: Decodable, Sendable {
        let items: [QueueJob]
    }

    /// Addresses the sender refuses to try again, as bare strings.
    struct SuppressionList: Decodable, Sendable {
        let items: [String]
    }

    /// Backend: `crates/webapi/src/handlers/dmarc.rs` — `ReportSummary`.
    ///
    /// A DMARC report is somebody else telling you what your mail
    /// looked like from their side, so `passing` against `total` is
    /// the deliverability number: mail that did not align was mail a
    /// receiver was entitled to reject.
    struct DmarcReport: Decodable, Identifiable, Sendable {
        let sid: String
        let orgName: String
        let policyDomain: String
        let begin: Int64
        let end: Int64
        /// The policy published at report time — `none`, `quarantine`
        /// or `reject`.
        let p: String
        let total: UInt64
        let passing: UInt64

        var id: String { sid }

        enum CodingKeys: String, CodingKey {
            case sid
            case orgName = "org_name"
            case policyDomain = "policy_domain"
            case begin
            case end
            case p
            case total
            case passing
        }
    }

    struct DmarcReportList: Decodable, Sendable {
        let items: [DmarcReport]
    }

    /// Backend: `SourceSummary` — one sending IP rolled up across
    /// reports. The ones that fail are the interesting ones: either a
    /// forwarder that breaks alignment, or somebody sending as you.
    struct DmarcSource: Decodable, Identifiable, Sendable {
        let sourceIp: String
        let total: UInt64
        let passing: UInt64
        let domains: [String]

        var id: String { sourceIp }

        enum CodingKeys: String, CodingKey {
            case sourceIp = "source_ip"
            case total
            case passing
            case domains
        }
    }

    struct DmarcSourceList: Decodable, Sendable {
        let items: [DmarcSource]
        let total: UInt64
        let passing: UInt64
        let reports: Int
    }

    /// Backend: `crates/core-api/src/method/admin/ops.rs` —
    /// `AuditRowWire`, served by `admin_audit.rs::list_audit_log`.
    ///
    /// Every admin write in this app records one of these, which is
    /// what makes the rest of the administration screens answerable
    /// afterwards rather than merely done.
    struct AuditRow: Decodable, Identifiable, Sendable {
        let id: Int64
        let timestamp: Int64
        /// Who did it, as an address.
        let actor: String
        /// Dotted, `alias.create` / `account.delete` — the prefix is
        /// what the server filters on.
        let action: String
        /// What it was done to.
        let target: String
        let detail: String
    }

    struct AuditList: Decodable, Sendable {
        let items: [AuditRow]
    }

    /// Backend: `crates/core-api/src/method/admin/permissions.rs` —
    /// `GroupWire`. A permission group, not an email group: this one
    /// decides who may do things, the other decides where mail goes.
    struct PermissionGroup: Decodable, Identifiable, Sendable {
        let id: Int64
        let name: String
        /// Absent for the cross-domain builtins.
        let domain: String?
        let description: String
        let isBuiltin: Bool
        let createdAt: Int64

        enum CodingKeys: String, CodingKey {
            case id
            case name
            case domain
            case description
            case isBuiltin = "is_builtin"
            case createdAt = "created_at"
        }
    }

    struct PermissionGroupList: Decodable, Sendable {
        let items: [PermissionGroup]
    }

    /// Both the group's grants and the server's catalogue arrive under
    /// `permissions` — the same key for two different questions, which
    /// is worth naming rather than reusing one type for.
    struct PermissionSet: Decodable, Sendable {
        let permissions: [String]
    }

    struct GroupMembers: Decodable, Sendable {
        let members: [String]
    }

    struct SetPermissionsRequest: Encodable, Sendable {
        let permissions: [String]
    }

    struct AddGroupRequest: Encodable, Sendable {
        let name: String
        let domain: String?
        let description: String
    }

    struct AliasList: Decodable, Sendable {
        let items: [Alias]
    }

    /// Backend: `AddAliasRequest`. `domain` is sent even though the
    /// server could split it off the source: the handler takes it as a
    /// field, and inferring what a server asks for is how a client
    /// starts disagreeing with it.
    struct AddAliasRequest: Encodable, Sendable {
        let sourceAddress: String
        let targetAddress: String
        let domain: String
        let aliasType: String

        enum CodingKeys: String, CodingKey {
            case sourceAddress = "source_address"
            case targetAddress = "target_address"
            case domain
            case aliasType = "alias_type"
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
