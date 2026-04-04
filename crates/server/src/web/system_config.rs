use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use super::{ApiResult, AuthUser, WebState};

/// GET /api/admin/system-config
pub(super) async fn list_config(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.system_config") {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "success": false,
                "message": "insufficient permissions"
            })),
        )
            .into_response();
    }

    let entries = match state.system_config {
        Some(ref store) => store.get_all_entries(),
        None => vec![],
    };

    Json(serde_json::json!({ "success": true, "entries": entries })).into_response()
}

#[derive(Deserialize)]
pub(super) struct UpdateConfigRequest {
    pub value: String,
}

/// PUT /api/admin/system-config/:key
pub(super) async fn update_config(
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Path(key): Path<String>,
    Json(req): Json<UpdateConfigRequest>,
) -> impl IntoResponse {
    if !permissions.has("admin.system_config") {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResult {
                success: false,
                message: Some("insufficient permissions".into()),
            }),
        )
            .into_response();
    }

    let store = match state.system_config {
        Some(ref s) => s,
        None => {
            return Json(ApiResult {
                success: false,
                message: Some("system config not available".into()),
            })
            .into_response();
        }
    };

    if let Err(e) = store.set(&key, &req.value, address).await {
        return Json(ApiResult {
            success: false,
            message: Some(e),
        })
        .into_response();
    }

    // audit log
    if let Some(ref ds) = state.domain_store {
        ds.log_audit(address, "system_config_updated", &key, &req.value)
            .await;
    }

    Json(ApiResult {
        success: true,
        message: None,
    })
    .into_response()
}

/// DELETE /api/admin/system-config/:key
pub(super) async fn reset_config(
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    if !permissions.has("admin.system_config") {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResult {
                success: false,
                message: Some("insufficient permissions".into()),
            }),
        )
            .into_response();
    }

    let store = match state.system_config {
        Some(ref s) => s,
        None => {
            return Json(ApiResult {
                success: false,
                message: Some("system config not available".into()),
            })
            .into_response();
        }
    };

    if let Err(e) = store.delete(&key).await {
        return Json(ApiResult {
            success: false,
            message: Some(e),
        })
        .into_response();
    }

    // audit log
    if let Some(ref ds) = state.domain_store {
        ds.log_audit(address, "system_config_reset", &key, "")
            .await;
    }

    Json(ApiResult {
        success: true,
        message: None,
    })
    .into_response()
}
