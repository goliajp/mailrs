use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
pub(crate) struct AddAliasRequest {
    pub source_address: String,
    pub target_address: String,
    pub domain: String,
    #[serde(default = "default_alias_type")]
    pub alias_type: String,
}

pub(crate) async fn list_aliases(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    Json(serde_json::to_value(ds.list_aliases().await.unwrap_or_default()).unwrap_or_default())
}

pub(crate) async fn add_alias(
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddAliasRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.aliases") {
        return err;
    }
    if req.source_address.is_empty() || req.source_address.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid source address length".into()),
        });
    }
    if req.target_address.is_empty() || req.target_address.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid target address length".into()),
        });
    }
    if req.domain.is_empty() || req.domain.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid domain length".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    let now = chrono::Utc::now().timestamp();
    match ds
        .add_alias(
            &req.source_address,
            &req.target_address,
            &req.domain,
            &req.alias_type,
            now,
        )
        .await
    {
        Ok(id) => {
            ds.log_audit(
                address,
                "alias_added",
                &id.to_string(),
                &format!(
                    "source={} target={} type={}",
                    req.source_address, req.target_address, req.alias_type
                ),
            )
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

pub(crate) async fn remove_alias(
    Path(id): Path<i64>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_alias(id).await {
        Ok(true) => {
            ds.log_audit(address, "alias_removed", &id.to_string(), "")
                .await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("alias not found".into()),
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
