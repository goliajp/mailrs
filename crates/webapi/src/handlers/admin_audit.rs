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
use crate::handlers::kevy_util::with_kevy;

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
        c.lrange(AUDIT_KEY, 0, scan - 1)
            .map_err(std::io::Error::from)
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
        .map(masked)
        .collect();
    Ok(Json(wire::AuditListResponse { items }))
}

/// GET /api/admin/audit-log/export?since=&until= — G12.5. Same
/// AuditRowWire array as `list_audit_log`, but the scan window is
/// unrestricted (no `limit` cap for time-window queries) so the
/// caller can dump a full 90-day span in one call. Actor / action
/// filters still apply.
pub async fn export_audit_log(
    _: crate::session::confirm::Confirmed,
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<wire::AuditListResponse>, StatusCode> {
    let entries = with_kevy(move |c| c.lrange(AUDIT_KEY, 0, -1).map_err(std::io::Error::from))?;
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
        .map(masked)
        .collect();
    Ok(Json(wire::AuditListResponse { items }))
}

/// Rows that name someone else's correspondent are shown with that part
/// hidden: a failed send keeps only the recipient's domain, and a thread
/// read shows a hash of the thread's Message-ID, which would otherwise
/// tell who writes to that mailbox. The stored row keeps the full value
/// for diagnosis on the host; only readers of the log see the hidden form.
fn shown(action: &str, field: &str, value: &str) -> Option<String> {
    match (action, field) {
        ("mail.send_failed", "target" | "detail") => Some(mask_addresses(value)),
        ("audit.read_thread", "detail") => Some(thread_hash(value)),
        _ => None,
    }
}

const MASKED_FIELDS: [&str; 2] = ["target", "detail"];

fn masked(mut row: wire::AuditRowWire) -> wire::AuditRowWire {
    if let Some(v) = shown(&row.action, "target", &row.target) {
        row.target = v;
    }
    if let Some(v) = shown(&row.action, "detail", &row.detail) {
        row.detail = v;
    }
    row
}

/// The same masking over a row read as loose JSON, for callers that pass
/// the stored fields through unchanged.
pub(crate) fn mask_json_row(row: &mut serde_json::Value) {
    let action = row
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or("")
        .to_string();
    for field in MASKED_FIELDS {
        if let Some(serde_json::Value::String(v)) = row.get_mut(field)
            && let Some(hidden) = shown(&action, field, v)
        {
            *v = hidden;
        }
    }
}

/// `message-id sha256:` and the first 16 hex digits of the id's SHA-256.
fn thread_hash(id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = hex::encode(Sha256::digest(id.as_bytes()));
    format!("message-id sha256:{}", &digest[..16])
}

/// Every `local@domain.tld` in `text` becomes `*@domain.tld`.
pub(crate) fn mask_addresses(text: &str) -> String {
    let local_char = |c: char| c.is_ascii_alphanumeric() || "._%+-'".contains(c);
    let domain_char = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        let (before, after) = rest.split_at(at);
        let after = &after[1..];
        let local_start = before
            .char_indices()
            .rev()
            .take_while(|&(_, c)| local_char(c))
            .last()
            .map(|(i, _)| i);
        let domain_len = after.find(|c: char| !domain_char(c)).unwrap_or(after.len());
        let domain = after[..domain_len].trim_end_matches(['.', '-']);
        match local_start {
            Some(start) if domain.contains('.') => {
                out.push_str(&before[..start]);
                out.push_str("*@");
                out.push_str(domain);
                rest = &after[domain.len()..];
            }
            _ => {
                out.push_str(before);
                out.push('@');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
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
    _: crate::session::confirm::Confirmed,
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

#[cfg(test)]
mod tests {
    use mailrs_core_api::method::admin::AuditRowWire;

    use super::{mask_addresses, mask_json_row, masked};
    use crate::handlers::audit::AuditFact;

    fn row(action: &str, target: &str, detail: &str) -> AuditRowWire {
        AuditRowWire {
            id: 1,
            timestamp: 1_790_000_000,
            actor: "system".into(),
            action: action.into(),
            target: target.into(),
            detail: detail.into(),
        }
    }

    #[test]
    fn a_failed_send_shows_only_the_correspondents_domain() {
        let got = masked(row(
            "mail.send_failed",
            "someone.else+tag@hotmail.com",
            "550 5.1.1 <someone.else+tag@hotmail.com>: unknown; reported by postmaster@mx.outlook.com. Contact abuse@outlook.com.",
        ));
        assert_eq!(got.target, "*@hotmail.com");
        assert_eq!(
            got.detail,
            "550 5.1.1 <*@hotmail.com>: unknown; reported by *@mx.outlook.com. Contact *@outlook.com."
        );
    }

    #[test]
    fn other_rows_and_non_addresses_are_left_alone() {
        let alias = masked(row(
            "alias.create",
            "research@golia.jp",
            "-> lihao@golia.jp",
        ));
        assert_eq!(alias.target, "research@golia.jp");
        assert_eq!(alias.detail, "-> lihao@golia.jp");
        assert_eq!(
            mask_addresses("a @ b, x@localhost, @y.z, q@"),
            "a @ b, x@localhost, @y.z, q@"
        );
    }

    #[test]
    fn a_thread_read_shows_a_labelled_hash_of_the_message_id() {
        let id = "20260918024741.9705133@mailap06.ib.mizuhobank.co.jp";
        let got = masked(row("audit.read_thread", "lihao@golia.jp", id));
        // sha256 of the id above, first 16 hex digits
        assert_eq!(got.detail, "message-id sha256:ff13553dfad8236b");
        assert_eq!(got.target, "lihao@golia.jp", "the mailbox read stays named");
        let mut v = serde_json::json!({"action": "audit.read_thread", "target": "a@b.example", "detail": id});
        mask_json_row(&mut v);
        assert_eq!(v["detail"], got.detail);
        let raw = masked(row("audit.read_raw", "lihao@golia.jp", "10069"));
        assert_eq!(raw.detail, "10069");
    }

    #[test]
    fn loose_json_rows_are_masked_the_same_way() {
        let mut v = serde_json::json!({
            "action": "mail.send_failed",
            "occurred_at": 1,
            "target": "a@b.example",
            "detail": "to a@b.example failed",
        });
        mask_json_row(&mut v);
        assert_eq!(v["target"], "*@b.example");
        assert_eq!(v["detail"], "to *@b.example failed");
        assert_eq!(v["occurred_at"], 1, "other fields pass through");
    }

    /// the row this crate's recorder writes must be one its reader keeps
    #[test]
    fn a_recorded_fact_reads_back_as_an_audit_row() {
        let fact = AuditFact {
            id: 12,
            occurred_at: 1_790_000_000,
            recorded_at: 1_790_000_000,
            actor: "admin@example.com".into(),
            action: "domain.create".into(),
            target: "example.org".into(),
            detail: String::new(),
        };
        let bytes = serde_json::to_vec(&fact).unwrap();
        let row: AuditRowWire = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(row.timestamp, 1_790_000_000);
        assert_eq!(row.action, "domain.create");
    }
}
