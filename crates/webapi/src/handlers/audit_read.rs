//! The audit **read** surface: reading another account's mail with the
//! access recorded. Every route here sits under `/api/admin/`, so the
//! admin middleware (`require_admin`) is the only gate.
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

/// Which account an audit read is about. `user` is the original name and
/// `target_user` the one the admin page and the MCP tools send; both work
/// so links already handed around keep resolving.
#[derive(Debug, serde::Deserialize)]
pub struct AuditConversationsQuery {
    #[serde(alias = "target_user")]
    pub user: Option<String>,
    #[serde(default = "default_audit_conv_limit")]
    pub limit: u32,
}

pub(crate) fn default_audit_conv_limit() -> u32 {
    100
}

fn core_status(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// GET /api/admin/audit/conversations?user=&limit= — list threads
/// for the target user via fastcore RPC. Same shape as normal
/// `/api/conversations` but scoped to any user.
pub async fn audit_conversations(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
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
        .map_err(core_status)?;
    crate::handlers::audit::record(&actor, "audit.list_conversations", &target, "via web");
    let items: Vec<serde_json::Value> = resp.items.into_iter().map(summary_json).collect();
    Ok(Json(serde_json::json!({
        "user": target,
        "items": items,
    })))
}

fn summary_json(c: mailrs_core_api::types::ConversationSummaryWire) -> serde_json::Value {
    serde_json::json!({
        "thread_id": c.thread_id,
        "subject": c.subject,
        "participants": c.participants,
        "message_count": c.message_count,
        "last_date": c.last_date,
        "snippet": c.snippet,
    })
}

#[derive(Debug, serde::Deserialize)]
pub struct AuditConvMessagesQuery {
    #[serde(alias = "target_user")]
    pub user: String,
}

/// GET /api/admin/audit/conversations/{thread_id}?user= — the thread's
/// summary row as the target user sees it, read through the core like
/// the list. The message list is `.../messages`.
pub async fn audit_conversation_detail(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<AuditConvMessagesQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let req = mailrs_core_api::method::conversation::ConversationsByIdsRequest {
        thread_ids: vec![thread_id.clone()],
        folder: None,
    };
    let resp = state
        .core
        .conversations_by_thread_ids(&q.user, &req)
        .await
        .map_err(core_status)?;
    let summary = resp.items.into_iter().next().ok_or(StatusCode::NOT_FOUND)?;
    crate::handlers::audit::record(&actor, "audit.read_thread", &q.user, &thread_id);
    Ok(Json(summary_json(summary)))
}

/// GET /api/admin/audit/conversations/{thread_id}/messages?user=
/// — the thread's messages as the target user has them, bodies and
/// attachment metadata included (the same enrichment the MCP
/// `audit_read_thread` tool returns).
pub async fn audit_conversation_messages(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<AuditConvMessagesQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let resp = state
        .core
        .list_thread_messages(&q.user, &thread_id)
        .await
        .map_err(core_status)?;
    crate::handlers::audit::record(&actor, "audit.read_thread", &q.user, &thread_id);
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let store = mailrs_message_store::MaildirStore;
    let mut items = Vec::with_capacity(resp.items.len());
    for w in resp.items {
        items.push(
            crate::handlers::conversations::enrich_with_body_public(
                &store,
                &maildir_root,
                &q.user,
                w,
            )
            .await,
        );
    }
    Ok(Json(serde_json::json!({
        "thread_id": thread_id,
        "user": q.user,
        "items": items,
    })))
}

/// GET /api/admin/audit/messages/{uid}/raw?user= — fetch raw envelope
/// bytes for a message under the target user. Reads maildir.
pub async fn audit_message_raw(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(uid): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<AuditConvMessagesQuery>,
) -> Result<axum::response::Response, StatusCode> {
    let wire = state
        .core
        .get_message_by_uid_for_user(&q.user, uid)
        .await
        .map_err(|e| StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::NOT_FOUND))?;
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let Some((path, id)) =
        crate::handlers::messages::blob_ref_location(&maildir_root, &q.user, &wire.blob_ref)
    else {
        return Err(StatusCode::NOT_FOUND);
    };
    use mailrs_message_store::MessageStore;
    match mailrs_message_store::MaildirStore.fetch(&path, &id).await {
        Ok(Some(bytes)) => {
            crate::handlers::audit::record(&actor, "audit.read_raw", &q.user, &uid.to_string());
            axum::response::Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "message/rfc822")
                .body(axum::body::Body::from(bytes))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        _ => Err(StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query<T: serde::de::DeserializeOwned>(qs: &str) -> T {
        let uri: axum::http::Uri = format!("/x?{qs}").parse().unwrap();
        axum::extract::Query::<T>::try_from_uri(&uri).unwrap().0
    }

    #[test]
    fn audit_queries_accept_user_and_target_user() {
        let by_user: AuditConversationsQuery = query("user=a%40b.c&limit=5");
        let by_target: AuditConversationsQuery = query("target_user=a%40b.c");
        assert_eq!(by_user.user.as_deref(), Some("a@b.c"));
        assert_eq!(by_user.limit, 5);
        assert_eq!(by_target.user.as_deref(), Some("a@b.c"));
        assert_eq!(by_target.limit, 100);
        let m: AuditConvMessagesQuery = query("target_user=x%40y.z");
        assert_eq!(m.user, "x@y.z");
        let m: AuditConvMessagesQuery = query("user=x%40y.z");
        assert_eq!(m.user, "x@y.z");
    }
}
