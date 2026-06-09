use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
pub(crate) struct SetOverridesRequest {
    pub overrides: Vec<OverrideEntry>,
}

#[derive(Deserialize)]
pub(crate) struct OverrideEntry {
    pub permission: String,
    pub granted: bool,
}

pub(crate) async fn get_account_groups(
    Path(address): Path<String>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.groups") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let groups = ds.get_account_groups(&address).await.unwrap_or_default();
    Json(serde_json::to_value(groups).unwrap_or_default())
}

pub(crate) async fn get_account_overrides(
    Path(address): Path<String>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.groups") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let overrides = ds.get_account_overrides(&address).await.unwrap_or_default();
    let entries: Vec<serde_json::Value> = overrides
        .into_iter()
        .map(|(perm, granted)| serde_json::json!({"permission": perm, "granted": granted}))
        .collect();
    Json(serde_json::json!(entries))
}

pub(crate) async fn set_account_overrides(
    Path(address): Path<String>,
    AuthUser {
        address: actor,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetOverridesRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    // validate permissions
    for entry in &req.overrides {
        if !crate::permission::ALL_PERMISSIONS.contains(&entry.permission.as_str()) {
            return Json(ApiResult {
                success: false,
                message: Some(format!("unknown permission: {}", entry.permission)),
            });
        }
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    let overrides: Vec<(String, bool)> = req
        .overrides
        .into_iter()
        .map(|e| (e.permission, e.granted))
        .collect();
    let detail = serde_json::json!({
        "address": address,
        "overrides": overrides
            .iter()
            .map(|(p, g)| serde_json::json!({"permission": p, "granted": g}))
            .collect::<Vec<_>>(),
    })
    .to_string();
    match ds.set_account_overrides(&address, &overrides).await {
        Ok(()) => {
            ds.log_audit(&actor, "account_overrides_set", &address, &detail)
                .await;
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
