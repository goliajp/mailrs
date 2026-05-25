use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;

// ---------- OAuth Client Admin ----------

#[derive(Deserialize)]
pub(crate) struct CreateOAuthClientRequest {
    pub name: String,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default = "default_oauth_scopes")]
    pub scopes: String,
    #[serde(default)]
    pub trusted: bool,
}

pub(crate) async fn create_oauth_client(
    AuthUser {
        ref permissions,
        ref address,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateOAuthClientRequest>,
) -> impl IntoResponse {
    if !permissions.has("admin.oauth_clients") {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"success": false, "message": "insufficient permissions"})),
        )
            .into_response();
    }
    if req.name.is_empty() || req.name.len() > MAX_ADMIN_FIELD_LEN {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "message": "invalid name length"})),
        )
            .into_response();
    }
    if req.redirect_uris.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"success": false, "message": "at least one redirect_uri required"}))).into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "message": "database not configured"})),
        )
            .into_response();
    };

    match crate::oidc_store::create_client(
        pool,
        &req.name,
        &req.redirect_uris,
        &req.scopes,
        req.trusted,
        address,
    )
    .await
    {
        Ok((client_id, secret)) => {
            tracing::info!(
                client = client_id.as_str(),
                name = req.name.as_str(),
                "oauth client created"
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "client_id": client_id,
                    "client_secret": secret,
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to create oauth client");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "message": "operation failed"})),
            )
                .into_response()
        }
    }
}

pub(crate) async fn list_oauth_clients(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.oauth_clients") {
        return Json(serde_json::json!([]));
    }
    let Some(ref pool) = state.pg_pool else {
        return Json(serde_json::json!([]));
    };
    let clients = crate::oidc_store::list_clients(pool)
        .await
        .unwrap_or_default();
    let result: Vec<serde_json::Value> = clients
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "client_id": c.client_id,
                "name": c.name,
                "redirect_uris": c.redirect_uris,
                "scopes": c.scopes,
                "trusted": c.trusted,
                "created_by": c.created_by,
                "created_at": c.created_at.to_rfc3339(),
            })
        })
        .collect();
    Json(serde_json::json!(result))
}

pub(crate) async fn delete_oauth_client(
    Path(client_id): Path<String>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.oauth_clients") {
        return err;
    }
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("database not configured".into()),
        });
    };
    match crate::oidc_store::delete_client(pool, &client_id).await {
        Ok(true) => {
            tracing::info!(client_id = client_id.as_str(), "oauth client deactivated");
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("client not found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "failed to delete oauth client");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
            })
        }
    }
}
