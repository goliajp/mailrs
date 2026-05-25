use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
pub(crate) struct AddAccountRequest {
    pub address: String,
    pub domain: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub recovery_email: String,
}

#[derive(Deserialize)]
pub(crate) struct UpdateAccountRequest {
    pub display_name: String,
}

pub(crate) async fn list_accounts(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return Json(Vec::<crate::domain_store::Account>::new());
    }
    let Some(ref ds) = state.domain_store else {
        return Json(Vec::<crate::domain_store::Account>::new());
    };
    Json(ds.list_accounts().await.unwrap_or_default())
}

pub(crate) async fn add_account(
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddAccountRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    if req.address.is_empty() || req.address.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid address length".into()),
        });
    }
    if req.domain.is_empty() || req.domain.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid domain length".into()),
        });
    }
    if req.display_name.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("display name too long".into()),
        });
    }
    if let Err(e) = crate::users::validate_email(&req.address) {
        return Json(ApiResult {
            success: false,
            message: Some(e.into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };

    // verify domain exists
    let domains = ds.list_domains().await.unwrap_or_default();
    if !domains.iter().any(|d| d.name == req.domain) {
        return Json(ApiResult {
            success: false,
            message: Some(format!("domain '{}' does not exist", req.domain)),
        });
    }

    // validate and hash password
    let password_hash = if req.password.is_empty() {
        String::new()
    } else {
        if let Err(e) = crate::users::validate_password(&req.password) {
            return Json(ApiResult {
                success: false,
                message: Some(e.into()),
            });
        }
        match crate::users::UserStore::hash_password(&req.password) {
            Ok(hash) => hash,
            Err(_) => {
                return Json(ApiResult {
                    success: false,
                    message: Some("failed to hash password".into()),
                });
            }
        }
    };

    let now = chrono::Utc::now().timestamp();
    match ds
        .add_account(
            &req.address,
            &req.domain,
            &req.display_name,
            &password_hash,
            now,
        )
        .await
    {
        Ok(()) => {
            // update recovery_email if provided
            if !req.recovery_email.is_empty() {
                let _ = ds
                    .update_recovery_email(&req.address, &req.recovery_email)
                    .await;
            }
            ds.log_audit(
                address,
                "account_created",
                &req.address,
                &format!("domain={}", req.domain),
            )
            .await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "admin add_account failed");
            let msg = match &e {
                crate::domain_store::StoreError::Pg(sqlx_err) => {
                    if let Some(db_err) = sqlx_err.as_database_error() {
                        if db_err.constraint() == Some("accounts_domain_fkey") {
                            format!("domain '{}' does not exist", req.domain)
                        } else if db_err.is_unique_violation() {
                            format!("account '{}' already exists", req.address)
                        } else {
                            "database error".into()
                        }
                    } else {
                        "database error".into()
                    }
                }
                _ => "operation failed".into(),
            };
            Json(ApiResult {
                success: false,
                message: Some(msg),
            })
        }
    }
}

pub(crate) async fn remove_account(
    Path(target_address): Path<String>,
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    // prevent admins from deleting their own account
    if target_address == *address {
        return Json(ApiResult {
            success: false,
            message: Some("cannot delete your own account".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_account(&target_address).await {
        Ok(true) => {
            ds.log_audit(address, "account_removed", &target_address, "")
                .await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
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

pub(crate) async fn update_account(
    Path(target_address): Path<String>,
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<UpdateAccountRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    if req.display_name.len() > MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("display name too long".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds
        .update_account_display_name(&target_address, &req.display_name)
        .await
    {
        Ok(true) => {
            ds.log_audit(
                address,
                "account_updated",
                &target_address,
                &format!("display_name={}", req.display_name),
            )
            .await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
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
