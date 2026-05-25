//! Spam-feedback (ham / spam) ingestion + per-user stats for ML training.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::{ApiResult, AuthUser, WebState};

#[derive(Deserialize)]
pub(crate) struct SpamFeedbackRequest {
    pub message_id: String,
    pub label: String, // "spam" or "ham"
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub sender: Option<String>,
}

pub(crate) async fn submit_spam_feedback(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SpamFeedbackRequest>,
) -> impl IntoResponse {
    if req.label != "spam" && req.label != "ham" {
        return Json(ApiResult {
            success: false,
            message: Some("label must be spam or ham".into()),
        })
        .into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("pg not available".into()),
        })
        .into_response();
    };

    match sqlx::query(
        "INSERT INTO spam_feedback (user_address, message_id, label, subject, sender) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&user)
    .bind(&req.message_id)
    .bind(&req.label)
    .bind(&req.subject)
    .bind(&req.sender)
    .execute(pool)
    .await
    {
        Ok(_) => Json(ApiResult {
            success: true,
            message: None,
        })
        .into_response(),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        })
        .into_response(),
    }
}

pub(crate) async fn get_spam_feedback_stats(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return Json(serde_json::json!({"error": "forbidden"})).into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return Json(serde_json::json!({"error": "pg not available"})).into_response();
    };
    let stats: Vec<(String, i64)> =
        sqlx::query_as("SELECT label, COUNT(*) FROM spam_feedback GROUP BY label")
            .fetch_all(pool)
            .await
            .unwrap_or_default();

    let total_spam = stats
        .iter()
        .find(|(l, _)| l == "spam")
        .map(|(_, c)| *c)
        .unwrap_or(0);
    let total_ham = stats
        .iter()
        .find(|(l, _)| l == "ham")
        .map(|(_, c)| *c)
        .unwrap_or(0);

    Json(serde_json::json!({
        "spam": total_spam,
        "ham": total_ham,
        "total": total_spam + total_ham,
    }))
    .into_response()
}
