use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
pub(crate) struct CreateGroupRequest {
    pub name: String,
    pub domain: Option<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize)]
pub(crate) struct SetGroupPermissionsRequest {
    pub permissions: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct AddMemberRequest {
    pub address: String,
}

pub(crate) async fn list_groups(
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
    let groups = ds.list_groups(None).await.unwrap_or_default();
    Json(serde_json::to_value(groups).unwrap_or_default())
}

pub(crate) async fn create_group(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateGroupRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    if req.name.is_empty() || req.name.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid group name length".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds
        .add_group(&req.name, req.domain.as_deref(), &req.description)
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

pub(crate) async fn delete_group(
    Path(id): Path<i64>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_group(id).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("group not found or is builtin".into()),
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

pub(crate) async fn get_group_permissions(
    Path(id): Path<i64>,
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
    let perms = ds.get_group_permissions(id).await.unwrap_or_default();
    Json(serde_json::json!(perms))
}

pub(crate) async fn set_group_permissions(
    Path(id): Path<i64>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetGroupPermissionsRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    // validate permissions
    for perm in &req.permissions {
        if !crate::permission::ALL_PERMISSIONS.contains(&perm.as_str()) {
            return Json(ApiResult {
                success: false,
                message: Some(format!("unknown permission: {perm}")),
            });
        }
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.set_group_permissions(id, &req.permissions).await {
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

pub(crate) async fn list_group_members(
    Path(id): Path<i64>,
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
    let members = ds.list_group_members(id).await.unwrap_or_default();
    Json(serde_json::json!(members))
}

pub(crate) async fn add_group_member(
    Path(id): Path<i64>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddMemberRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.add_account_to_group(&req.address, id).await {
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

pub(crate) async fn remove_group_member(
    Path((id, address)): Path<(i64, String)>,
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_account_from_group(&address, id).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("membership not found".into()),
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
