import Foundation

/// Running the server — the queue, DMARC, the audit log, API keys.
///
/// Split out of `Wire.swift` when it passed the 500-line limit this
/// repository holds every language to. Same namespace, same shapes —
/// only the file they live in changed.
extension Wire {

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


    /// Backend: `crates/webapi/src/handlers/apps_keys.rs` —
    /// `list_agent_keys`, `GET /api/agent/keys`, in an `{items: […]}`
    /// envelope.
    ///
    /// The secret is **not** here and cannot be: the server stores only
    /// its first eight characters, so `prefix` is the whole of what a
    /// key can be recognised by after the moment it is made.
    struct AgentKey: Decodable, Identifiable, Sendable {
        let id: Int64
        let name: String
        let scopes: [String]
        let prefix: String
        let createdAt: Int64

        enum CodingKeys: String, CodingKey {
            case id
            case name
            case scopes
            case prefix
            case createdAt = "created_at"
        }
    }


    struct CreateAgentKeyRequest: Encodable {
        let name: String
        let scopes: [String]
    }


    /// The one and only time the secret exists outside the caller.
    struct CreateAgentKeyResponse: Decodable {
        let id: Int64
        let secret: String
    }
}
