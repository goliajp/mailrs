//! Audit, system config, maildir reconcile, export.

use crate::types::UserAddress;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRowWire {
    pub id: i64,
    /// webapi's recorder and fastcore's system events write the fact
    /// time as `occurred_at`; older rows carry `timestamp`
    #[serde(alias = "occurred_at")]
    pub timestamp: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogAuditRequest {
    pub actor: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ListAuditQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: u32,
}

fn default_audit_limit() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditListResponse {
    pub items: Vec<AuditRowWire>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CleanupAuditRequest {
    /// Delete rows older than this many days.
    pub older_than_days: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CleanupAuditResponse {
    pub deleted: u32,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — domains
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfigRow {
    pub key: String,
    pub value: String,
    /// `string` / `int` / `bool` / `float` / `json`.
    pub value_type: String,
    pub updated_at: i64,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfigListResponse {
    pub items: Vec<SystemConfigRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSystemConfigRequest {
    pub value: String,
    pub value_type: String,
    pub updated_by: String,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — reconcile + backfill + export
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct ReconcileRequest {
    /// `true` = report the gap without writing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub scanned: u64,
    pub missing: u64,
    pub repaired: u64,
    /// Per-mailbox error messages.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileResponse {
    pub dry_run: bool,
    pub report: ReconcileReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExportRequest {
    /// Required: user to export.
    pub user: UserAddress,
    /// Optional epoch-seconds lower bound (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<i64>,
    /// Optional epoch-seconds upper bound (exclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
    /// Optional ILIKE-style filter over subject + text_body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Max rows.
    #[serde(default = "default_export_limit")]
    pub limit: u32,
}

fn default_export_limit() -> u32 {
    1000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedMessageRow {
    pub message_id: String,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub internal_date: i64,
    pub size: u32,
    pub text_body: String,
    pub folder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResponse {
    pub items: Vec<ExportedMessageRow>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — API keys
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::AuditRowWire;

    #[test]
    fn audit_rows_read_in_both_stored_shapes() {
        let recorded = br#"{"id":7,"occurred_at":1790000000,"recorded_at":1790000001,"actor":"system","action":"mail.bounce","target":"a@b.c","detail":"DSN delivered to sender"}"#;
        let legacy = br#"{"id":3,"timestamp":1780000000,"actor":"x@y.z","action":"auth.login","target":"x@y.z","detail":""}"#;
        let r: AuditRowWire = serde_json::from_slice(recorded).unwrap();
        assert_eq!(
            (r.id, r.timestamp, r.action.as_str()),
            (7, 1_790_000_000, "mail.bounce")
        );
        let l: AuditRowWire = serde_json::from_slice(legacy).unwrap();
        assert_eq!((l.id, l.timestamp), (3, 1_780_000_000));
        let out = serde_json::to_value(&r).unwrap();
        assert_eq!(out["timestamp"], 1_790_000_000);
        assert!(out.get("occurred_at").is_none());
    }
}
