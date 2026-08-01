//! The audit log, and the exports built from it.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
};
use mailrs_core_api::method::admin as wire;

use crate::WebState;
use crate::handlers::admin::*;
use crate::handlers::conversations::AuthedUser;

#[derive(Debug, serde::Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: u32,
    /// Filter by actor (exact email) — G12.4.
    pub actor: Option<String>,
    /// Filter by action prefix (e.g. "account" matches account.*).
    pub action: Option<String>,
    /// Time-window lower bound (Unix seconds, inclusive) — G12.5. When
    /// set together with `until`, powers the JSON export endpoint.
    #[serde(default)]
    pub since: Option<i64>,
    /// Time-window upper bound (Unix seconds, exclusive) — G12.5.
    #[serde(default)]
    pub until: Option<i64>,
}

pub(crate) fn default_audit_limit() -> u32 {
    100
}

/// GET /api/admin/audit-log
pub async fn list_audit_log(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<wire::AuditListResponse>, StatusCode> {
    let limit = q.limit as usize;
    // read a wider window when filtering so the caller still gets up to
    // `limit` matching rows, not `limit` pre-filter rows
    let scan = if q.actor.is_some() || q.action.is_some() {
        10_000
    } else {
        limit as i64
    };
    let entries = with_kevy(move |c| {
        c.lrange(AUDIT_KEY.as_bytes(), 0, scan - 1)
            .map_err(std::io::Error::other)
    })?;
    let items: Vec<wire::AuditRowWire> = entries
        .into_iter()
        .filter_map(|v| serde_json::from_slice::<wire::AuditRowWire>(&v).ok())
        .filter(|row| q.actor.as_deref().is_none_or(|a| row.actor == a))
        .filter(|row| {
            q.action
                .as_deref()
                .is_none_or(|a| row.action.starts_with(a))
        })
        .filter(|row| q.since.is_none_or(|s| row.timestamp >= s))
        .filter(|row| q.until.is_none_or(|u| row.timestamp < u))
        .take(limit)
        .collect();
    Ok(Json(wire::AuditListResponse { items }))
}

/// GET /api/admin/audit-log/export?since=&until= — G12.5. Same
/// AuditRowWire array as `list_audit_log`, but the scan window is
/// unrestricted (no `limit` cap for time-window queries) so the
/// caller can dump a full 90-day span in one call. Actor / action
/// filters still apply.
pub async fn export_audit_log(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<wire::AuditListResponse>, StatusCode> {
    let entries = with_kevy(move |c| {
        c.lrange(AUDIT_KEY.as_bytes(), 0, -1)
            .map_err(std::io::Error::other)
    })?;
    let items: Vec<wire::AuditRowWire> = entries
        .into_iter()
        .filter_map(|v| serde_json::from_slice::<wire::AuditRowWire>(&v).ok())
        .filter(|row| q.actor.as_deref().is_none_or(|a| row.actor == a))
        .filter(|row| {
            q.action
                .as_deref()
                .is_none_or(|a| row.action.starts_with(a))
        })
        .filter(|row| q.since.is_none_or(|s| row.timestamp >= s))
        .filter(|row| q.until.is_none_or(|u| row.timestamp < u))
        .collect();
    Ok(Json(wire::AuditListResponse { items }))
}

#[derive(Debug, serde::Deserialize)]
pub struct AdminExportQuery {
    pub user: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// GET /api/admin/export?user=&limit= — stream a JSONL blob of the
/// user's threads (subject + participants + message_ids). Full raw
/// export via `audit_message_raw`.
///
/// Access model: any admin can export their own account; only super
/// admins can export somebody else's data. Enforced explicitly here so
/// the middleware layer (which just checks admin.*) can't be
/// side-stepped by passing `?user=someone_else`.
pub async fn admin_export(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(caller)): Extension<AuthedUser>,
    Query(q): Query<AdminExportQuery>,
) -> Result<axum::response::Response, StatusCode> {
    if q.user != caller {
        let perms = state
            .core
            .effective_permissions(&caller)
            .await
            .map_err(|_| StatusCode::FORBIDDEN)?;
        if !perms.is_super {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    let limit = q.limit.unwrap_or(1000).min(10_000);
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit,
            ..Default::default()
        },
    };
    let resp = state
        .core
        .list_conversations(&q.user, &req)
        .await
        .map_err(map_err)?;
    let mut lines = String::new();
    for c in resp.items {
        let line = serde_json::json!({
            "thread_id": c.thread_id,
            "subject": c.subject,
            "participants": c.participants,
            "message_count": c.message_count,
            "unread_count": c.unread_count,
            "last_date": c.last_date,
            "category": c.category,
        })
        .to_string();
        lines.push_str(&line);
        lines.push('\n');
    }
    let filename = format!("export-{}.jsonl", q.user);
    let response = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/jsonl")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(axum::body::Body::from(lines))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(response)
}
