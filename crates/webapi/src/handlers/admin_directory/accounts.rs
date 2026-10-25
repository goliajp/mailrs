//! Accounts: create, edit, password, quota, overrides.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use mailrs_core_api::method::admin as wire;
use mailrs_core_sidestate::families::permission_groups;

use crate::WebState;
use crate::handlers::admin::map_err;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// GET /api/admin/accounts
pub async fn list_accounts(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::AccountListResponse>, StatusCode> {
    state.core.list_accounts().await.map(Json).map_err(map_err)
}

/// POST /api/admin/accounts — provision a new account in fastcore, which
/// owns the embedded kevy accounts live in. 409 when the address exists:
/// creating never rewrites an account.
pub async fn add_account(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<wire::AddAccountRequest>,
) -> Result<axum::response::Response, StatusCode> {
    use axum::response::IntoResponse;
    if let Err(weak) = mailrs_auth_guard::password::check_new(&req.password, &req.address) {
        return Ok(crate::handlers::auth::password::refused(weak));
    }
    state.core.add_account(&req).await.map_err(map_err)?;
    crate::handlers::audit::record(&actor, "account.create", &req.address, "");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /api/admin/accounts/{address} — remove the account in fastcore,
/// then end every session and agent key the address holds.
pub async fn remove_account(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state.core.remove_account(&address).await.map_err(map_err)?;
    // after the account is gone, so no sign-in can slip in behind it
    crate::handlers::auth::revoke_all_access(&address)?;
    crate::handlers::audit::record(&actor, "account.delete", &address, "");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
pub struct SetAccountPasswordRequest {
    pub password: String,
}

/// POST /api/admin/accounts/{address}/password — replace only the
/// password hash; quota, recovery email and permissions stay as they are.
pub async fn set_account_password(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<SetAccountPasswordRequest>,
) -> Result<axum::response::Response, StatusCode> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng},
    };
    use axum::response::IntoResponse;
    if let Err(weak) = mailrs_auth_guard::password::check_new(&req.password, &address) {
        return Ok(crate::handlers::auth::password::refused(weak));
    }
    let salt = SaltString::generate(&mut ArgonRng);
    let password_hash = Argon2::default()
        .hash_password(req.password.as_bytes(), &salt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    state
        .core
        .set_account_password(&address, &wire::SetPasswordRequest { password_hash })
        .await
        .map_err(map_err)?;
    // whoever signed in with the old password is signed out; agent keys
    // are separate credentials and stay
    let addr = address.clone();
    crate::handlers::kevy_util::with_kevy(move |c| {
        crate::handlers::auth::revoke_sessions(c, &addr)
    })?;
    crate::handlers::audit::record(&actor, "account.password_reset", &address, "");
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateAccountRequest {
    pub display_name: Option<String>,
    pub recovery_email: Option<String>,
}

/// PUT /api/admin/accounts/{address} — patch account fields via
/// fastcore.
pub async fn update_account(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut fields_changed = Vec::new();
    if let Some(dn) = req.display_name {
        let wire_req = wire::UpdateAccountRequest {
            display_name: dn.clone(),
        };
        state
            .core
            .update_account(&address, &wire_req)
            .await
            .map_err(map_err)?;
        fields_changed.push(format!("display_name={dn}"));
    }
    if let Some(re) = req.recovery_email {
        let wire_req = wire::UpdateRecoveryEmailRequest {
            recovery_email: re.clone(),
        };
        state
            .core
            .set_recovery_email(&address, &wire_req)
            .await
            .map_err(map_err)?;
        fields_changed.push(format!("recovery_email={re}"));
    }
    if !fields_changed.is_empty() {
        crate::handlers::audit::record(
            &actor,
            "account.update",
            &address,
            &fields_changed.join(","),
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/accounts/{address}/quota — the stored quota in bytes,
/// read from fastcore, which holds the account. 404 for no such account.
pub async fn get_account_quota(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<wire::GetQuotaResponse>, StatusCode> {
    let acc = state
        .core
        .get_account_with_hash(&address)
        .await
        .map_err(map_err)?;
    Ok(Json(wire::GetQuotaResponse {
        quota_bytes: acc.public.quota_bytes,
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct SetQuotaRequest {
    pub quota_bytes: i64,
}

/// POST /api/admin/accounts/{address}/quota — patch `quota_bytes` via
/// fastcore RPC. `-1` sentinel means unlimited.
pub async fn set_account_quota(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<SetQuotaRequest>,
) -> Result<StatusCode, StatusCode> {
    let wire_req = wire::SetQuotaRequest {
        quota_bytes: req.quota_bytes,
    };
    state
        .core
        .set_quota(&address, &wire_req)
        .await
        .map_err(map_err)?;
    crate::handlers::audit::record(
        &actor,
        "account.quota",
        &address,
        &format!("{} bytes", req.quota_bytes),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/accounts/{address}/groups — names of the permission
/// groups whose member set holds the account.
pub async fn list_account_groups(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let held = with_kevy(move |c| {
        permission_groups::groups_of(c, &address).map_err(std::io::Error::from)
    })?;
    let groups: Vec<String> = held.into_iter().map(|g| g.name).collect();
    Ok(Json(serde_json::json!({ "groups": groups })))
}

pub async fn get_account_overrides(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:account:{address}:overrides");
    let val = with_kevy(move |c| c.get(key.as_bytes()).map_err(std::io::Error::from))?;
    let parsed: serde_json::Value = val
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    Ok(Json(parsed))
}

pub async fn set_account_overrides(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:account:{address}:overrides");
    let payload = serde_json::to_vec(&req).map_err(|_| StatusCode::BAD_REQUEST)?;
    let payload_c = payload.clone();
    with_kevy(move |c| {
        c.set(key.as_bytes(), &payload_c)?;
        Ok(())
    })?;
    crate::handlers::audit::record(
        &actor,
        "overrides.update",
        &address,
        &String::from_utf8(payload).unwrap_or_default(),
    );
    Ok(StatusCode::NO_CONTENT)
}
