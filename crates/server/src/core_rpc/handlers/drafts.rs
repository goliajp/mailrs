//! Drafts CRUD — direct SQL through `state.pool` (drafts have no
//! dedicated stone crate; the SQL lives in
//! `crates/server/src/web/mail/drafts.rs` and is mirrored here without
//! touching the existing handler).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/drafts
pub async fn list_drafts(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::DraftListResponse>, StatusCode> {
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            i64,
            i64,
        ),
    >(
        "SELECT id, to_addresses, cc_addresses, bcc_addresses, subject, body, \
                reply_to_thread_id, EXTRACT(EPOCH FROM created_at)::bigint, \
                EXTRACT(EPOCH FROM updated_at)::bigint \
         FROM drafts WHERE user_address = $1 ORDER BY updated_at DESC",
    )
    .bind(&user)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, user = %user, "list_drafts failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let items = rows
        .into_iter()
        .map(
            |(id, to, cc, bcc, subject, body, reply, created, updated)| wire::DraftWire {
                id,
                to,
                cc,
                bcc,
                subject,
                body,
                reply_to_thread_id: reply,
                created_at: created,
                updated_at: updated,
            },
        )
        .collect();
    Ok(Json(wire::DraftListResponse { items }))
}

/// POST /v1/users/{user}/drafts
pub async fn save_draft(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::SaveDraftRequest>,
) -> Result<Json<wire::SaveDraftResponse>, StatusCode> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO drafts (user_address, to_addresses, cc_addresses, bcc_addresses, \
                             subject, body, reply_to_thread_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(&user)
    .bind(&req.to)
    .bind(&req.cc)
    .bind(&req.bcc)
    .bind(&req.subject)
    .bind(&req.body)
    .bind(&req.reply_to_thread_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, user = %user, "save_draft failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::SaveDraftResponse { id }))
}

/// DELETE /v1/users/{user}/drafts/{id}
pub async fn delete_draft(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, id)): Path<(String, i64)>,
) -> Result<StatusCode, StatusCode> {
    let result = sqlx::query("DELETE FROM drafts WHERE id = $1 AND user_address = $2")
        .bind(id)
        .bind(&user)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, id, "delete_draft failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if result.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}
