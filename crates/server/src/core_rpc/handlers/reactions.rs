//! Reactions GET + toggle — direct SQL through `state.pool`.
//! Mirrors `web/conversations/mutations/actions.rs` toggle semantics
//! without modifying that file (ironrule).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/threads/{thread_id}/reactions
pub async fn get_thread_reactions(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ReactionsResponse>, StatusCode> {
    let rows = sqlx::query_as::<_, (i64, String, i64, bool)>(
        "SELECT message_uid, emoji, COUNT(*)::bigint AS count, \
                BOOL_OR(account_address = $2) AS me \
         FROM reactions WHERE thread_id = $1 \
         GROUP BY message_uid, emoji \
         ORDER BY message_uid, emoji",
    )
    .bind(&thread_id)
    .bind(&user)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, thread_id = %thread_id, "get_thread_reactions failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let reactions = rows
        .into_iter()
        .map(
            |(message_uid, emoji, count, me)| wire::ReactionAggregateRow {
                message_uid,
                emoji,
                count,
                me,
            },
        )
        .collect();
    Ok(Json(wire::ReactionsResponse { reactions }))
}

/// PUT /v1/users/{user}/threads/{thread_id}/messages/{uid}/reactions
///
/// Toggle: insert; on conflict the row already existed so delete instead.
pub async fn toggle_reaction(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id, uid)): Path<(String, String, i64)>,
    Json(req): Json<wire::ToggleReactionRequest>,
) -> Result<Json<wire::ReactionsResponse>, StatusCode> {
    if req.emoji.is_empty() || req.emoji.len() > 32 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let inserted = sqlx::query_scalar::<_, bool>(
        "INSERT INTO reactions (message_uid, thread_id, account_address, emoji) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (message_uid, account_address, emoji) DO NOTHING \
         RETURNING true",
    )
    .bind(uid)
    .bind(&thread_id)
    .bind(&user)
    .bind(&req.emoji)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, "toggle_reaction insert failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if inserted.is_none() {
        sqlx::query(
            "DELETE FROM reactions WHERE message_uid = $1 AND account_address = $2 AND emoji = $3",
        )
        .bind(uid)
        .bind(&user)
        .bind(&req.emoji)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "toggle_reaction delete failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    // Return the fresh thread-wide aggregate so the client doesn't need a
    // follow-up GET. Same shape as get_thread_reactions.
    get_thread_reactions(State(state), Path((user, thread_id))).await
}
