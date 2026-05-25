use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
pub(crate) struct CreateAppRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// comma-separated scopes or array
    pub scopes: String,
}

#[derive(Deserialize)]
pub(crate) struct UpdateAppScopesRequest {
    pub scopes: String,
}

pub(crate) async fn list_apps(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let apps = ds.list_apps(None).await.unwrap_or_default();
    Json(serde_json::to_value(apps).unwrap_or_default())
}

pub(crate) async fn create_app(
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateAppRequest>,
) -> impl IntoResponse {
    if require_permission(permissions, "admin.accounts").is_some() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }
    if req.name.is_empty() || req.name.len() > MAX_ADMIN_FIELD_LEN {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid app name"})),
        )
            .into_response();
    }
    // validate scopes
    let scopes: Vec<&str> = req
        .scopes
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    for scope in &scopes {
        if !crate::permission::ALL_PERMISSIONS.contains(scope) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("unknown scope: {scope}")})),
            )
                .into_response();
        }
    }

    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "domain store not configured"})),
        )
            .into_response();
    };

    let app_id = uuid::Uuid::new_v4().to_string();
    let scopes_str = scopes.join(",");

    match ds
        .create_app(&app_id, &req.name, &req.description, address, &scopes_str)
        .await
    {
        Ok(id) => {
            ds.log_audit(
                address,
                "app_created",
                &req.name,
                &format!("app_id={app_id}"),
            )
            .await;
            // generate an initial API key for the app
            let Some(ref pool) = state.pg_pool else {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "db unavailable"})),
                )
                    .into_response();
            };

            let (full_key, prefix, key_hash) = crate::api_key_store::generate_api_key();
            match crate::api_key_store::insert_app_api_key(
                pool, &prefix, &key_hash, &full_key, address, &req.name, id, None,
            ).await {
                Ok(key_id) => {
                    (StatusCode::CREATED, Json(serde_json::json!({
                        "app_id": app_id,
                        "name": req.name,
                        "scopes": scopes_str,
                        "api_key": {
                            "id": key_id,
                            "key": full_key,
                            "prefix": prefix,
                        },
                    }))).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("app created but key generation failed: {e}")}))).into_response()
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "operation failed"})),
            )
        }
        .into_response(),
    }
}

pub(crate) async fn get_app(
    Path(app_id): Path<String>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "domain store not configured"})),
        )
            .into_response();
    };
    match ds.get_app(&app_id).await {
        Ok(Some(app)) => (
            StatusCode::OK,
            Json(serde_json::to_value(app).unwrap_or_default()),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "app not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "operation failed"})),
            )
        }
        .into_response(),
    }
}

pub(crate) async fn delete_app(
    Path(app_id): Path<String>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_app(&app_id).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("app not found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
            })
        }
    }
}

pub(crate) async fn update_app_scopes(
    Path(app_id): Path<String>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<UpdateAppScopesRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    // validate scopes
    let scopes: Vec<&str> = req
        .scopes
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    for scope in &scopes {
        if !crate::permission::ALL_PERMISSIONS.contains(scope) {
            return Json(ApiResult {
                success: false,
                message: Some(format!("unknown scope: {scope}")),
            });
        }
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.update_app_scopes(&app_id, &scopes.join(",")).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("app not found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
            })
        }
    }
}
