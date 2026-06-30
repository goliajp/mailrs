//! Outbound queue endpoints — sender ↔ core.
//!
//! Source: `crates/outbound-queue/src/queue.rs` (15+ fn).
//!
//! These are the **critical RPC surface for §4 sender split** — sender
//! claims jobs from core, marks outcomes, manages suppression.

use serde::{Deserialize, Serialize};

// ── paths ───────────────────────────────────────────────────────────

pub const PATH_ENQUEUE: &str = "/v1/outbound/enqueue";
pub const PATH_ENQUEUE_SCHEDULED: &str = "/v1/outbound/scheduled";
pub const PATH_CLAIM: &str = "/v1/outbound/claim";
pub const PATH_RECOVER_STALE: &str = "/v1/outbound/recover-stale";
pub const PATH_STATS: &str = "/v1/outbound/stats";
pub const PATH_MARK_INFLIGHT: &str = "/v1/outbound/{id}/inflight";
pub const PATH_MARK_DELIVERED: &str = "/v1/outbound/{id}/delivered";
pub const PATH_MARK_FAILED: &str = "/v1/outbound/{id}/failed";
pub const PATH_MARK_BOUNCED: &str = "/v1/outbound/{id}/bounced";
pub const PATH_RETRY: &str = "/v1/outbound/{id}/retry";
pub const PATH_GET_MESSAGE: &str = "/v1/outbound/{id}";
pub const PATH_RECENT: &str = "/v1/outbound/recent";
pub const PATH_CANCEL_PENDING: &str = "/v1/outbound/{id}";
pub const PATH_IS_SUPPRESSED: &str = "/v1/outbound/suppression/{email}";
pub const PATH_ADD_SUPPRESSION: &str = "/v1/outbound/suppression";
pub const PATH_LIST_SUPPRESSIONS: &str = "/v1/outbound/suppression";
pub const PATH_REMOVE_SUPPRESSION: &str = "/v1/outbound/suppression/{email}";
pub const PATH_TLS_RPT_EVENT: &str = "/v1/outbound/tls-rpt-events";

// ── wire types ──────────────────────────────────────────────────────

/// One outbound queue row — what `sender` gets back from `:claim`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessageWire {
    /// Store-native primary key.
    pub id: i64,
    /// Envelope sender (SMTP MAIL FROM).
    pub sender: String,
    /// Envelope recipient.
    pub recipient: String,
    /// Optional original sender pre-SRS (for forwarded mail).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_sender: Option<String>,
    /// Raw message bytes, base64-encoded.
    pub message_data_base64: String,
    /// Current status (see `QueueStatus`).
    pub status: QueueStatus,
    /// Attempts so far.
    pub attempts: u32,
    /// Last delivery error, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Epoch seconds of next retry attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_retry: Option<i64>,
    /// Epoch seconds when scheduled to send (None = immediately).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<i64>,
    /// Epoch seconds when row was created.
    pub created_at: i64,
    /// Epoch seconds when row was last updated.
    pub updated_at: i64,
}

/// Outbound queue status enum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    Pending,
    Inflight,
    Delivered,
    Failed,
    Bounced,
}

// ── req/resp ────────────────────────────────────────────────────────

/// Request body for `POST /v1/outbound/enqueue`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnqueueRequest {
    pub sender: String,
    pub recipient: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_sender: Option<String>,
    /// Raw RFC 5322 message bytes, base64.
    pub message_data_base64: String,
    /// Optional schedule time, epoch seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<i64>,
}

/// Response — the newly-assigned id.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnqueueResponse {
    /// Store-native id of the enqueued row.
    pub id: i64,
}

/// Request body for `POST /v1/outbound/claim`.
///
/// Sender atomically claims N pending rows via `FOR UPDATE SKIP LOCKED`
/// (PG) or `WATCH/MULTI` (kevy). Each claimed row is set to `inflight`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimRequest {
    /// Max rows to claim.
    pub batch_size: u32,
}

/// Response — the rows now owned by the calling sender.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimResponse {
    pub items: Vec<OutboundMessageWire>,
}

/// Request body for `POST /v1/outbound/recover-stale`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RecoverStaleRequest {
    /// Reset to `pending` any `inflight` rows older than this many seconds.
    pub older_than_secs: u64,
}

/// Response — how many were recovered.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoverStaleResponse {
    pub recovered: u32,
}

/// Response body for `GET /v1/outbound/stats`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct QueueStatsResponse {
    pub pending: i64,
    pub inflight: i64,
    pub delivered: i64,
    pub failed: i64,
    pub bounced: i64,
}

/// Request body for `POST /v1/outbound/{id}/failed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkFailedRequest {
    /// Human-readable error to record.
    pub error: String,
    /// Epoch-seconds for the next retry attempt (None = use default backoff).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_retry: Option<i64>,
}

/// Request body for `POST /v1/outbound/{id}/bounced`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkBouncedRequest {
    /// Final bounce error.
    pub error: String,
}

/// Request body for `POST /v1/outbound/suppression`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddSuppressionRequest {
    /// Email address to suppress (lowercased server-side).
    pub email: String,
    /// Why — `hard_bounce` / `complaint` / `manual` / ...
    pub reason: String,
}

/// Response body for `GET /v1/outbound/suppression/{email}`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct IsSuppressedResponse {
    /// `true` if this email is on the suppression list.
    pub suppressed: bool,
}

/// Response body for `GET /v1/outbound/recent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentOutboundResponse {
    /// Most recent N rows, ordered by `created_at` DESC.
    pub items: Vec<OutboundMessageWire>,
}

/// Request body for `POST /v1/outbound/tls-rpt-events`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsRptEventRequest {
    /// Either `success` or `failure`.
    pub kind: String,
    /// Recipient policy domain (the receiving side).
    pub policy_domain: String,
    /// JSON metadata for the event (success counts / failure detail / etc.).
    pub detail: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_status_lowercase() {
        let s = serde_json::to_string(&QueueStatus::Pending).unwrap();
        assert_eq!(s, "\"pending\"");
        let back: QueueStatus = serde_json::from_str("\"delivered\"").unwrap();
        assert_eq!(back, QueueStatus::Delivered);
    }

    #[test]
    fn enqueue_request_minimal() {
        let req = EnqueueRequest {
            sender: "a@x.com".into(),
            recipient: "b@y.com".into(),
            original_sender: None,
            message_data_base64: "aGVsbG8=".into(),
            scheduled_at: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(!s.contains("original_sender"));
        assert!(!s.contains("scheduled_at"));
    }

    #[test]
    fn claim_response_empty() {
        let r = ClaimResponse { items: vec![] };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, "{\"items\":[]}");
    }

    #[test]
    fn stats_default_zero() {
        let s = QueueStatsResponse::default();
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"pending\":0"));
    }
}
