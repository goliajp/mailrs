//! Handlers for `mailrs_core_api::method::mailbox`.
//!
//! Eight endpoints — CRUD + status + ensure_default.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::mailbox as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/mailboxes
pub async fn list_mailboxes(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::ListMailboxesResponse>, StatusCode> {
    let rows = state.mailbox.list_mailboxes(&user).await.map_err(|e| {
        tracing::warn!(error = %e, user = %user, "list_mailboxes failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows.iter().map(Into::into).collect();
    Ok(Json(wire::ListMailboxesResponse { items }))
}

/// GET /v1/users/{user}/mailboxes/{name}
pub async fn get_mailbox(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, name)): Path<(String, String)>,
) -> Result<Json<wire::MailboxWire>, StatusCode> {
    let row = state
        .mailbox
        .get_mailbox(&user, &name)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, name = %name, "get_mailbox failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json((&row).into()))
}

/// GET /v1/mailboxes/{id}
pub async fn get_mailbox_by_id(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<Json<wire::MailboxWire>, StatusCode> {
    let row = state
        .mailbox
        .get_mailbox_by_id(id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, id, "get_mailbox_by_id failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json((&row).into()))
}

/// POST /v1/users/{user}/mailboxes
pub async fn create_mailbox(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::CreateMailboxRequest>,
) -> Result<Json<wire::CreateMailboxResponse>, StatusCode> {
    let row = state
        .mailbox
        .create_mailbox(&user, &req.name)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, name = %req.name, "create_mailbox failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::CreateMailboxResponse {
        mailbox: (&row).into(),
    }))
}

/// DELETE /v1/users/{user}/mailboxes/{name}
pub async fn delete_mailbox(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, name)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let removed = state
        .mailbox
        .delete_mailbox(&user, &name)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, name = %name, "delete_mailbox failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// POST /v1/users/{user}/mailboxes/{name}/rename
pub async fn rename_mailbox(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, name)): Path<(String, String)>,
    Json(req): Json<wire::RenameMailboxRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .rename_mailbox(&user, &name, &req.to)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, from = %name, to = %req.to, "rename failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /v1/mailboxes/{id}/status
pub async fn mailbox_status(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<Json<wire::MailboxStatusResponse>, StatusCode> {
    let (total, unread) = state.mailbox.mailbox_status(id).await.map_err(|e| {
        tracing::warn!(error = %e, id, "mailbox_status failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::MailboxStatusResponse {
        status: wire::MailboxStatusWire {
            total,
            unread,
            recent: 0,
        },
    }))
}
