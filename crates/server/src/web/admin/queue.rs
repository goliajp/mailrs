use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use super::*;

#[derive(serde::Serialize)]
pub(crate) struct QueueEntry {
    pub id: i64,
    pub sender: String,
    pub recipient: String,
    pub domain: String,
    pub status: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub(crate) struct RetryResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Deserialize)]
pub(crate) struct SuppressionAction {
    pub email: String,
}

pub(crate) async fn get_queue(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref pool) = state.outbound_queue else {
        return Json(Vec::<QueueEntry>::new());
    };

    let entries = match mailrs_outbound_queue::queue::list_recent(pool, 100).await {
        Ok(msgs) => msgs
            .into_iter()
            .map(|m| QueueEntry {
                id: m.id,
                sender: m.sender,
                recipient: m.recipient,
                domain: m.domain,
                status: m.status.as_str().to_string(),
                attempts: m.attempts,
                last_error: m.last_error,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })
            .collect(),
        Err(_) => vec![],
    };

    Json(entries)
}

pub(crate) async fn retry_queue_message(
    Path(id): Path<i64>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.outbound_queue else {
        return Json(RetryResponse {
            success: false,
            message: "outbound queue not configured".into(),
        });
    };

    let now = chrono::Utc::now().timestamp();
    match mailrs_outbound_queue::queue::retry_message(pool, id, now).await {
        Ok(true) => Json(RetryResponse {
            success: true,
            message: format!("message {id} queued for retry"),
        }),
        Ok(false) => Json(RetryResponse {
            success: false,
            message: format!("message {id} not found or not retryable"),
        }),
        Err(e) => Json(RetryResponse {
            success: false,
            message: format!("error: {e}"),
        }),
    }
}

// --- suppression list ---

pub(crate) async fn list_suppressed(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(resp) = require_permission(permissions, "admin.queue") {
        return resp.into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return Json(serde_json::json!({"error": "pg not available"})).into_response();
    };
    match mailrs_outbound_queue::queue::list_suppressions(pool, 500).await {
        Ok(items) => {
            let list: Vec<_> = items.into_iter().map(|(email, reason, code, ts)| {
                serde_json::json!({ "email": email, "reason": reason, "smtp_code": code, "created_at": ts })
            }).collect();
            Json(serde_json::json!({ "suppressions": list })).into_response()
        }
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })).into_response(),
    }
}

pub(crate) async fn remove_suppressed(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(body): Json<SuppressionAction>,
) -> impl IntoResponse {
    if let Some(resp) = require_permission(permissions, "admin.queue") {
        return resp.into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult { success: false, message: Some("pg not available".into()) }).into_response();
    };
    match mailrs_outbound_queue::queue::remove_suppression(pool, &body.email).await {
        Ok(true) => Json(ApiResult { success: true, message: None }).into_response(),
        Ok(false) => Json(ApiResult { success: false, message: Some("not found".into()) }).into_response(),
        Err(e) => Json(ApiResult { success: false, message: Some(e.to_string()) }).into_response(),
    }
}
