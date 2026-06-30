//! `/api/mail/*` REST handlers.
//!
//! Phase 3 — thin shims over `state.core_client.X()`.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::{mailbox as mb_wire, message as msg_wire};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// GET /api/mail/folders
pub async fn get_folders(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<mb_wire::ListMailboxesResponse>, StatusCode> {
    state
        .core_client
        .list_mailboxes(&user)
        .await
        .map(Json)
        .map_err(map_err)
}

/// GET /api/mail/messages/{uid}
///
/// Phase 3 partial: today's REST API resolves the mailbox by name from
/// session context, but the shim takes mailbox_id via query for now.
/// Full path-compat handler will land with checklist 3.6.
#[derive(Debug, serde::Deserialize)]
pub struct UidQuery {
    pub mailbox_id: i64,
}

pub async fn get_message(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(uid): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<UidQuery>,
) -> Result<Json<msg_wire::MessageWire>, StatusCode> {
    state
        .core_client
        .get_message_by_uid(q.mailbox_id, uid)
        .await
        .map(Json)
        .map_err(map_err)
}

/// GET /api/mail/stats
///
/// Combines unseen-count + action-count + total messages into a
/// dashboard-shaped response. webapi assembles it in-process via two
/// RPC calls so the existing frontend payload shape is preserved.
#[derive(Debug, serde::Serialize)]
pub struct MailStatsResponse {
    pub unseen: i64,
    pub action: i64,
}

pub async fn get_mail_stats(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<MailStatsResponse>, StatusCode> {
    let unseen = state
        .core_client
        .unseen_count(&user)
        .await
        .map_err(map_err)?;
    let action = state
        .core_client
        .action_count(&user)
        .await
        .map_err(map_err)?;
    Ok(Json(MailStatsResponse {
        unseen: unseen.count,
        action: action.count,
    }))
}
