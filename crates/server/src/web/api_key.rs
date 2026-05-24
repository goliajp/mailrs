use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api_key_store;

use super::auth::{AuthMethod, AuthUser};
use super::WebState;

#[derive(Deserialize)]
pub(super) struct CreateApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct CreateApiKeyResponse {
    id: i64,
    key: String,
    prefix: String,
    name: String,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct ApiKeyResponse {
    id: i64,
    key: Option<String>,
    prefix: String,
    name: String,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
}

/// POST /api/agent/keys — create a new API key (session auth only)
pub(super) async fn create_api_key(
    State(state): State<Arc<WebState>>,
    auth_user: AuthUser,
    Json(req): Json<CreateApiKeyRequest>,
) -> impl IntoResponse {
    // only session-authenticated users can create API keys
    if matches!(auth_user.auth_method, AuthMethod::ApiKey(_)) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "api keys cannot create other api keys"})),
        );
    }

    // validate name
    if req.name.is_empty() || req.name.len() > 100 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name must be 1-100 characters"})),
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

    // retry up to 3 times on prefix collision
    for _ in 0..3 {
        let (full_key, prefix, key_hash) = api_key_store::generate_api_key();

        match api_key_store::insert_api_key(
            pool,
            &prefix,
            &key_hash,
            &full_key,
            &auth_user.address,
            &req.name,
            req.expires_at,
        )
        .await
        {
            Ok(id) => {
                return (
                    StatusCode::CREATED,
                    Json(serde_json::json!(CreateApiKeyResponse {
                        id,
                        key: full_key,
                        prefix,
                        name: req.name,
                        expires_at: req.expires_at,
                    })),
                );
            }
            Err(e) => {
                // check for unique constraint violation (prefix collision)
                let msg = e.to_string();
                if msg.contains("unique") || msg.contains("duplicate") {
                    continue;
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "failed to create api key"})),
                );
            }
        }
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "failed to generate unique key prefix"})),
    )
}

/// GET /api/agent/keys — list all active API keys for the authenticated user
pub(super) async fn list_api_keys(
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

    match api_key_store::list_api_keys(pool, &auth_user.address).await {
        Ok(records) => {
            let keys: Vec<ApiKeyResponse> = records
                .into_iter()
                .map(|r| ApiKeyResponse {
                    id: r.id,
                    key: r.full_key,
                    prefix: r.prefix,
                    name: r.name,
                    created_at: r.created_at,
                    expires_at: r.expires_at,
                    last_used_at: r.last_used_at,
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!(keys)))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to list api keys"})),
        ),
    }
}

/// DELETE /api/agent/keys/{id} — revoke an API key
pub(super) async fn revoke_api_key(
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

    match api_key_store::revoke_api_key(pool, id, &auth_user.address).await {
        Ok(Some(prefix)) => {
            // evict from Valkey cache
            if let Some(ref valkey) = state.valkey {
                api_key_store::cache_delete(valkey, &prefix).await;
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"success": true})),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "api key not found or already revoked"})),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to revoke api key"})),
        ),
    }
}

#[cfg(test)]
#[path = "api_key_tests.rs"]
mod tests;
