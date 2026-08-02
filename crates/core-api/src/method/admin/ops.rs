//! Audit, system config, maildir reconcile, export.

use crate::types::UserAddress;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRowWire {
    pub id: i64,
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
