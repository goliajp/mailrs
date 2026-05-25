use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
pub(crate) struct CreateEmailGroupRequest {
    pub address: String,
    pub domain: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

pub(crate) async fn list_email_groups(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let groups = ds.list_email_groups(None).await.unwrap_or_default();
    Json(serde_json::to_value(groups).unwrap_or_default())
}

pub(crate) async fn create_email_group(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateEmailGroupRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    if req.address.is_empty() || req.address.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid address".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds
        .create_email_group(&req.address, &req.domain, &req.name, &req.description)
        .await
    {
        Ok(id) => Json(ApiResult {
            success: true,
            message: Some(id.to_string()),
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

pub(crate) async fn delete_email_group(
    Path(id): Path<i64>,
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
    match ds.remove_email_group(id).await {
        Ok(Some(_)) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(None) => Json(ApiResult {
            success: false,
            message: Some("group not found".into()),
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

pub(crate) async fn list_email_group_members(
    Path(id): Path<i64>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let members = ds.list_email_group_members(id).await.unwrap_or_default();
    Json(serde_json::json!(members))
}

pub(crate) async fn add_email_group_member(
    Path(id): Path<i64>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddMemberRequest>,
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
    // prevent adding the group's own address as a member (would cause infinite delivery)
    if let Ok(groups) = ds.list_email_groups(None).await
        && let Some(group) = groups.iter().find(|g| g.id == id)
        && group.address == req.address
    {
        return Json(ApiResult {
            success: false,
            message: Some("cannot add group as member of itself".into()),
        });
    }
    match ds.add_email_group_member(id, &req.address).await {
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
        }
    }
}

pub(crate) async fn remove_email_group_member(
    Path((id, address)): Path<(i64, String)>,
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
    match ds.remove_email_group_member(id, &address).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("member not found".into()),
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
