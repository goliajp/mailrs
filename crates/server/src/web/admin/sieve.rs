use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Serialize)]
pub(crate) struct SieveResponse {
    pub address: String,
    pub script: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SetSieveRequest {
    pub script: String,
}

pub(crate) async fn get_sieve(
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
    match ds.get_sieve_script(&address).await {
        Ok(script) => Json(SieveResponse { address, script }).into_response(),
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

pub(crate) async fn set_sieve(
    Path(address): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetSieveRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.sieve") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    if req.script.len() > MAX_SIEVE_SCRIPT_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("sieve script too large".into()),
        });
    }
    // validate sieve script before saving
    if let Err(e) = mailrs_sieve::compile_sieve(&req.script) {
        return Json(ApiResult {
            success: false,
            message: Some(format!("invalid sieve script: {e}")),
        });
    }
    let now = chrono::Utc::now().timestamp();
    match ds.set_sieve_script(&address, &req.script, now).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(crate) async fn delete_sieve(
    Path(address): Path<String>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.delete_sieve_script(&address).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("no sieve script found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}
