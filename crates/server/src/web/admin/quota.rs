use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Serialize)]
pub(crate) struct QuotaResponse {
    pub address: String,
    pub quota_bytes: i64,
}

#[derive(Deserialize)]
pub(crate) struct SetQuotaRequest {
    pub quota_bytes: i64,
}

pub(crate) async fn get_quota(
    Path(address): Path<String>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "domain store not configured"})),
        )
            .into_response();
    };
    match ds.get_quota(&address).await {
        Ok(Some(quota_bytes)) => Json(QuotaResponse {
            address,
            quota_bytes,
        })
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "account not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "operation failed"})),
            )
                .into_response()
        }
    }
}

pub(crate) async fn set_quota(
    Path(address): Path<String>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetQuotaRequest>,
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
    match ds.set_quota(&address, req.quota_bytes).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("account not found".into()),
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
