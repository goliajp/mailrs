//! Action-dismiss + feedback + reaction toggle.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;


use super::super::super::{ApiResult, AuthUser, WebState};
use super::super::*;

pub(crate) async fn dismiss_action(
    Path(thread_id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ApiResult { success: false, message: Some("thread id too long".into()) });
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.dismiss_thread_action(&user, &thread_id).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: Some("action dismissed".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}


pub(crate) async fn record_feedback(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<FeedbackRequest>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    if req.sender_email.len() > 320 || !req.sender_email.contains('@') {
        return Json(ApiResult {
            success: false,
            message: Some("invalid sender email".into()),
        });
    }

    if !VALID_FEEDBACK_ACTIONS.contains(&req.action.as_str()) {
        return Json(ApiResult {
            success: false,
            message: Some(format!(
                "invalid action, must be one of: {}",
                VALID_FEEDBACK_ACTIONS.join(", ")
            )),
        });
    }

    match mb_store
        .record_sender_feedback(&user, &req.sender_email, &req.action)
        .await
    {
        Ok(()) => Json(ApiResult {
            success: true,
            message: Some(format!("feedback '{}' recorded", req.action)),
        }),
        Err(e) => {
            tracing::error!(event = "feedback_error", user = %user, error = %e);
            Json(ApiResult {
                success: false,
                message: Some("internal error".into()),
            })
        }
    }
}


pub(crate) async fn toggle_reaction(
    Path((thread_id, uid)): Path<(String, i64)>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<ToggleReactionRequest>,
) -> impl IntoResponse {
    if thread_id.len() > crate::web::MAX_PATH_LEN {
        return Json(ToggleReactionResponse { reactions: vec![] });
    }

    // validate emoji: at most 32 bytes, non-empty
    if req.emoji.is_empty() || req.emoji.len() > 32 {
        return Json(ToggleReactionResponse { reactions: vec![] });
    }

    let Some(ref pool) = state.pg_pool else {
        return Json(ToggleReactionResponse { reactions: vec![] });
    };

    // toggle: try insert, if conflict then delete
    let inserted = sqlx::query_scalar::<_, bool>(
        "INSERT INTO reactions (message_uid, thread_id, account_address, emoji)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (message_uid, account_address, emoji) DO NOTHING
         RETURNING true"
    )
    .bind(uid)
    .bind(&thread_id)
    .bind(&user)
    .bind(&req.emoji)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if inserted.is_none() {
        // row already existed — remove it
        let _ = sqlx::query(
            "DELETE FROM reactions WHERE message_uid = $1 AND account_address = $2 AND emoji = $3"
        )
        .bind(uid)
        .bind(&user)
        .bind(&req.emoji)
        .execute(pool)
        .await;
    }

    // fetch updated reactions for this message
    let reactions = super::super::queries::fetch_message_reactions(pool, uid, &user).await;
    Json(ToggleReactionResponse { reactions })
}
