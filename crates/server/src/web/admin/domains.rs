use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
pub(crate) struct AddDomainRequest {
    pub name: String,
}

pub(crate) async fn list_domains(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(Vec::<crate::domain_store::Domain>::new());
    };
    Json(ds.list_domains().await.unwrap_or_default())
}

pub(crate) async fn add_domain(
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddDomainRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.domains") {
        return err;
    }
    if req.name.is_empty() || req.name.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid domain name length".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    let now = chrono::Utc::now().timestamp();
    match ds.add_domain(&req.name, now).await {
        Ok(()) => {
            ds.log_audit(address, "domain_added", &req.name, "").await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
            })
        }
    }
}

pub(crate) async fn remove_domain(
    Path(name): Path<String>,
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.domains") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_domain(&name).await {
        Ok(true) => {
            ds.log_audit(address, "domain_removed", &name, "").await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("domain not found".into()),
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

pub(crate) async fn check_domain_handler(
    Path(name): Path<String>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref resolver) = state.resolver else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "DNS resolver not available"})),
        );
    };
    let pm_resolver = mailrs_postmaster::HickoryPostmasterResolver::new((**resolver).clone());
    let report = mailrs_postmaster::check_domain(
        &pm_resolver,
        &name,
        state.dkim_selector.as_deref(),
        &state.hostname,
    )
    .await;
    match serde_json::to_value(report) {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::json!({"error": format!("failed to serialize domain check report: {e}")}),
            ),
        ),
    }
}

// --- domain reputation ---

pub(crate) async fn get_reputation(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(resp) = require_permission(permissions, "admin.domains") {
        return resp.into_response();
    }
    let Some(ref pool) = state.outbound_queue else {
        return Json(serde_json::json!({"error": "queue not available"})).into_response();
    };
    let rep = crate::reputation::compute_reputation(pool).await;
    Json(serde_json::json!({ "domains": rep })).into_response()
}

// --- RBL blocklist status ---

pub(crate) async fn get_rbl_status(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(resp) = require_permission(permissions, "admin.domains") {
        return resp.into_response();
    }

    let Some(ref kevy) = state.kevy else {
        return Json(serde_json::json!({"error": "kevy not available"})).into_response();
    };

    // scan for rbl:status:* keys
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg("rbl:status:*")
        .query_async(&mut kevy.clone())
        .await
        .unwrap_or_default();

    let mut reports = Vec::new();
    for key in &keys {
        if let Ok(json) = redis::cmd("GET")
            .arg(key)
            .query_async::<String>(&mut kevy.clone())
            .await
            && let Ok(report) = serde_json::from_str::<serde_json::Value>(&json)
        {
            reports.push(report);
        }
    }

    Json(serde_json::json!({ "reports": reports })).into_response()
}
