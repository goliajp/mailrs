use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::webhook::store;

use super::auth::AuthUser;
use super::WebState;

/// maximum url length for webhook endpoints
const MAX_URL_LEN: usize = 2048;

#[derive(Deserialize)]
pub(super) struct CreateWebhookRequest {
    pub url: String,
    #[serde(default = "default_event_type")]
    pub event_type: String,
    #[serde(default)]
    pub filter_sender: Option<String>,
    #[serde(default)]
    pub filter_thread_id: Option<String>,
}

fn default_event_type() -> String {
    "new_message".to_string()
}

#[derive(Serialize)]
struct CreateWebhookResponse {
    id: i64,
    url: String,
    event_type: String,
    filter_sender: Option<String>,
    filter_thread_id: Option<String>,
    signing_secret: String,
    active: bool,
    warning: &'static str,
}

#[derive(Serialize)]
struct WebhookListItem {
    id: i64,
    url: String,
    event_type: String,
    filter_sender: Option<String>,
    filter_thread_id: Option<String>,
    active: bool,
    created_at: String,
}

/// validate webhook url: must be https (or http for localhost/127.0.0.1)
fn is_valid_webhook_url(url: &str) -> bool {
    if url.len() > MAX_URL_LEN {
        return false;
    }
    if url.starts_with("https://") {
        return true;
    }
    if let Some(after_scheme) = url.strip_prefix("http://") {
        // allow http only for localhost development
        return after_scheme.starts_with("localhost") || after_scheme.starts_with("127.0.0.1");
    }
    false
}

/// POST /api/agent/webhooks — create a new webhook subscription
pub(super) async fn create_webhook(
    State(state): State<Arc<WebState>>,
    auth_user: AuthUser,
    Json(req): Json<CreateWebhookRequest>,
) -> impl IntoResponse {
    // validate url
    if !is_valid_webhook_url(&req.url) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "url must start with https:// (or http:// for localhost)"})),
        );
    }

    // validate event_type
    if req.event_type != "new_message" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "event_type must be 'new_message'"})),
        );
    }

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database unavailable"})),
            )
        }
    };

    let signing_secret = store::generate_signing_secret();

    match store::create_subscription(
        pool,
        &auth_user.address,
        &req.url,
        &req.event_type,
        req.filter_sender.as_deref(),
        req.filter_thread_id.as_deref(),
        &signing_secret,
    )
    .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!(CreateWebhookResponse {
                id,
                url: req.url,
                event_type: req.event_type,
                filter_sender: req.filter_sender,
                filter_thread_id: req.filter_thread_id,
                signing_secret,
                active: true,
                warning: "Save this signing secret now. It cannot be retrieved again.",
            })),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to create webhook subscription"})),
        ),
    }
}

/// GET /api/agent/webhooks — list all active webhook subscriptions
pub(super) async fn list_webhooks(
    State(state): State<Arc<WebState>>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database unavailable"})),
            )
        }
    };

    match store::list_subscriptions(pool, &auth_user.address).await {
        Ok(subs) => {
            let items: Vec<WebhookListItem> = subs
                .into_iter()
                .map(|s| WebhookListItem {
                    id: s.id,
                    url: s.url,
                    event_type: s.event_type,
                    filter_sender: s.filter_sender,
                    filter_thread_id: s.filter_thread_id,
                    active: s.active,
                    created_at: s.created_at.to_rfc3339(),
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!(items)))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to list webhook subscriptions"})),
        ),
    }
}

/// DELETE /api/agent/webhooks/{id} — delete a webhook subscription
pub(super) async fn delete_webhook(
    State(state): State<Arc<WebState>>,
    auth_user: AuthUser,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database unavailable"})),
            )
        }
    };

    match store::delete_subscription(pool, id, &auth_user.address).await {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"success": true})),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "webhook not found or already deleted"})),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to delete webhook subscription"})),
        ),
    }
}

#[cfg(test)]
#[path = "webhook_tests.rs"]
mod tests;
