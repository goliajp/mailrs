//! Signatures CRUD — direct SQL through `state.pool`. Same pattern as
//! `drafts.rs`; the existing handler at
//! `crates/server/src/web/mail/signatures.rs` is not modified.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/signatures
pub async fn list_signatures(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::SignatureListResponse>, StatusCode> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, bool, String)>(
        "SELECT id, name, html, text_content, is_default, \
                to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
         FROM signatures WHERE account_address = $1 ORDER BY created_at",
    )
    .bind(&user)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, user = %user, "list_signatures failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let items = rows
        .into_iter()
        .map(
            |(id, name, html, text_content, is_default, created_at)| wire::SignatureWire {
                id,
                name,
                html,
                text_content,
                is_default,
                created_at,
            },
        )
        .collect();
    Ok(Json(wire::SignatureListResponse { items }))
}

/// POST /v1/users/{user}/signatures
pub async fn save_signature(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::SaveSignatureRequest>,
) -> Result<Json<wire::SaveSignatureResponse>, StatusCode> {
    // If setting as default, unset any existing default first (matches
    // monolith behavior).
    if req.is_default {
        sqlx::query("UPDATE signatures SET is_default = false WHERE account_address = $1")
            .bind(&user)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, user = %user, "save_signature unset-default failed");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO signatures (account_address, name, html, text_content, is_default) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(&user)
    .bind(&req.name)
    .bind(&req.html)
    .bind(&req.text_content)
    .bind(req.is_default)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, user = %user, "save_signature insert failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::SaveSignatureResponse { id }))
}

/// DELETE /v1/users/{user}/signatures/{id}
pub async fn delete_signature(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, id)): Path<(String, i64)>,
) -> Result<StatusCode, StatusCode> {
    let result = sqlx::query("DELETE FROM signatures WHERE id = $1 AND account_address = $2")
        .bind(id)
        .bind(&user)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, id, "delete_signature failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if result.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}
