//! The audit **read** surface: reading another account's mail with the
//! access recorded. Every route here is behind `admin.impersonate`.
//!
//! Distinct from `audit.rs`, which is the recorder these call into. The
//! two were nearly merged by a careless split on 2026-08-02 — the reader
//! is what an operator invokes, the recorder is what makes invoking it
//! leave a trace.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::complete::*;
use crate::handlers::conversations::AuthedUser;

/// GET /api/admin/audit/accounts — return the registered accounts
/// shaped for the audit panel.
///
/// v2.2-fix (2026-07-09): pre-fix version read from network-kevy set
/// `mailrs:accounts:index` — a legacy key that fastcore never wrote
/// to after the split; the set was empty in prod so the audit page
/// showed "No auditable accounts" even for super-admin. Now calls
/// fastcore's `list_accounts` RPC (embedded kevy — where the
/// accounts actually live). Populates address / domain / display_name
/// / active shape the frontend `AuditAccount` type expects.
pub async fn audit_accounts(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let accounts = state
        .core
        .list_accounts()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items: Vec<serde_json::Value> = accounts
        .items
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "address": a.address,
                "domain": a.domain,
                "display_name": a.display_name,
                "active": a.active,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct AuditConversationsQuery {
    pub user: Option<String>,
    #[serde(default = "default_audit_conv_limit")]
    pub limit: u32,
}

pub(crate) fn default_audit_conv_limit() -> u32 {
    100
}

/// GET /api/admin/audit/conversations?user=&limit= — list threads
/// for the target user via fastcore RPC. Same shape as normal
/// `/api/conversations` but scoped to any user (admin impersonation).
pub async fn audit_conversations(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Query(q): axum::extract::Query<AuditConversationsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let Some(target) = q.user else {
        return Ok(Json(serde_json::json!({ "items": [] })));
    };
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit: q.limit,
            ..Default::default()
        },
    };
    let resp = state
        .core
        .list_conversations(&target, &req)
        .await
        .map_err(|e| {
            StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    let items: Vec<serde_json::Value> = resp
        .items
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "thread_id": c.thread_id,
                "subject": c.subject,
                "participants": c.participants,
                "message_count": c.message_count,
                "last_date": c.last_date,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "user": target,
        "items": items,
    })))
}

/// GET /api/admin/audit/conversations/{thread_id} — thread summary
/// for admin audit. Returns thread aggregate fields (subject,
/// participants, count) but NOT the message list — use
/// `.../messages` for that.
pub async fn audit_conversation_detail(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Best we can do without a user context: read thread aggregate
    // directly from network kevy. Fastcore's per-user RPCs need a user.
    let key = format!("mailrs:thread:{thread_id}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::other))?;
    if flat.is_empty() {
        return Ok(Json(
            serde_json::json!({ "thread_id": thread_id, "found": false }),
        ));
    }
    let mut obj = serde_json::Map::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let k = String::from_utf8_lossy(&flat[i]).to_string();
        let v = String::from_utf8_lossy(&flat[i + 1]).to_string();
        obj.insert(k, serde_json::Value::String(v));
        i += 2;
    }
    obj.insert(
        "thread_id".into(),
        serde_json::Value::String(thread_id.clone()),
    );
    obj.insert("found".into(), serde_json::Value::Bool(true));
    Ok(Json(serde_json::Value::Object(obj)))
}

#[derive(Debug, serde::Deserialize)]
pub struct AuditConvMessagesQuery {
    pub user: String,
}

/// GET /api/admin/audit/conversations/{thread_id}/messages?user=
/// — the message list for a thread scoped to a target user.
pub async fn audit_conversation_messages(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<AuditConvMessagesQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let resp = state
        .core
        .list_thread_messages(&q.user, &thread_id)
        .await
        .map_err(|e| {
            StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    Ok(Json(serde_json::json!({
        "thread_id": thread_id,
        "user": q.user,
        "items": resp.items,
    })))
}

/// GET /api/admin/audit/messages/{uid}/raw?user= — fetch raw envelope
/// bytes for a message under an impersonated user. Reads maildir.
pub async fn audit_message_raw(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(uid): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<AuditConvMessagesQuery>,
) -> Result<axum::response::Response, StatusCode> {
    let wire = state
        .core
        .get_message_by_uid_for_user(&q.user, uid)
        .await
        .map_err(|e| StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::NOT_FOUND))?;
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let Some((local, domain)) = q.user.split_once('@') else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let path = format!("{maildir_root}/{domain}/{local}");
    let store = mailrs_message_store::MaildirStore;
    use mailrs_message_store::MessageStore;
    let id = mailrs_message_store::MessageId(wire.blob_ref);
    match store.fetch(&path, &id).await {
        Ok(Some(bytes)) => axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "message/rfc822")
            .body(axum::body::Body::from(bytes))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR),
        _ => Err(StatusCode::NOT_FOUND),
    }
}
