//! Re-authentication and password change — the endpoints that ask for
//! a credential the caller already has.

//! `/api/auth/*` REST handlers.

use std::sync::Arc;

use argon2::Argon2;
use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use password_hash::{PasswordHash, PasswordVerifier};
use serde::Deserialize;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::login_guard::ClientIp;
use mailrs_auth_guard::login::Verdict;

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub address: String,
    pub password: String,
    /// The address of the person signing in to the calling service, so
    /// the login guard counts them rather than the service. Without it
    /// the service's own address is counted.
    #[serde(default)]
    pub client_ip: Option<std::net::IpAddr>,
}

#[derive(Debug, Deserialize)]
pub struct VerifyTotpRequest {
    pub address: String,
    pub code: String,
    /// As in [`VerifyRequest`].
    #[serde(default)]
    pub client_ip: Option<std::net::IpAddr>,
}

fn perm_check_internal_rpc(perms: &[String]) -> bool {
    perms.iter().any(|p| p == "internal.rpc" || p == "*")
}

/// POST /api/auth/verify — password verification without session
/// creation (internal RPC only). Same wire shape as monolith.
pub async fn verify_credentials(
    State(state): State<Arc<WebState>>,
    ClientIp(caller_ip): ClientIp,
    Extension(AuthedUser(caller)): Extension<AuthedUser>,
    Json(req): Json<VerifyRequest>,
) -> Response {
    let perms = state
        .core
        .effective_permissions(&caller)
        .await
        .map(|p| p.permissions)
        .unwrap_or_default();
    if !perm_check_internal_rpc(&perms) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "permission denied"})),
        )
            .into_response();
    }
    if req.address.len() > 256 || req.password.len() > 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid input length"})),
        )
            .into_response();
    }
    let source = req.client_ip.unwrap_or(caller_ip);
    let guard = &*crate::login_guard::GUARD;
    let verdict = guard.check(source, &req.address).await;
    if let Verdict::Locked { .. } = verdict {
        tracing::warn!(event = "auth", channel = "verify", %caller, %source, account = %mailrs_auth_guard::login::loggable_account(&req.address), result = "locked");
        return crate::login_guard::locked_response(verdict);
    }
    // an unknown account and a wrong password answer alike, so the
    // endpoint cannot be used to find out which accounts exist
    let acct = match super::password::verify(&state, &req.address, &req.password).await {
        Ok(Some(acct)) => acct,
        Ok(None) => {
            tracing::warn!(event = "auth", channel = "verify", %caller, %source, account = %mailrs_auth_guard::login::loggable_account(&req.address), result = "bad-password");
            guard.failed(source, &req.address).await;
            return (
                StatusCode::OK,
                Json(serde_json::json!({"valid": false, "reason": "invalid_credentials"})),
            )
                .into_response();
        }
        Err(status) => return status.into_response(),
    };
    guard.succeeded(source, &req.address).await;
    let totp_key = format!("totp:{}", req.address);
    let totp_required = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(totp_key.as_bytes(), b"enabled")
            .map_err(std::io::Error::from)
    })
    .ok()
    .flatten()
    .map(|v| v == b"1")
    .unwrap_or(false);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "valid": true,
            "display_name": acct.public.display_name,
            "domain": acct.public.address.split_once('@').map(|(_, d)| d).unwrap_or(""),
            "totp_required": totp_required,
        })),
    )
        .into_response()
}

/// POST /api/auth/verify-totp — internal-rpc TOTP code check (used by
/// external IdP integrations to defer 2FA to mailrs).
pub async fn verify_totp(
    State(state): State<Arc<WebState>>,
    ClientIp(caller_ip): ClientIp,
    Extension(AuthedUser(caller)): Extension<AuthedUser>,
    Json(req): Json<VerifyTotpRequest>,
) -> Response {
    let perms = state
        .core
        .effective_permissions(&caller)
        .await
        .map(|p| p.permissions)
        .unwrap_or_default();
    if !perm_check_internal_rpc(&perms) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "permission denied"})),
        )
            .into_response();
    }
    if req.address.len() > 256 || req.code.len() > 32 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid input length"})),
        )
            .into_response();
    }
    let source = req.client_ip.unwrap_or(caller_ip);
    let guard = &*crate::login_guard::GUARD;
    let verdict = guard.check(source, &req.address).await;
    if let Verdict::Locked { .. } = verdict {
        tracing::warn!(event = "auth", channel = "verify", %caller, %source, account = %mailrs_auth_guard::login::loggable_account(&req.address), result = "locked");
        return crate::login_guard::locked_response(verdict);
    }
    let key = format!("totp:{}", req.address);
    let key_r = key.clone();
    let secret = match crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(key_r.as_bytes(), b"secret")
            .map_err(std::io::Error::from)
    }) {
        Ok(Some(v)) => String::from_utf8(v).unwrap_or_default(),
        _ => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({"valid": false, "reason": "totp_not_configured"})),
            )
                .into_response();
        }
    };
    let enabled_key = key.clone();
    let enabled = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(enabled_key.as_bytes(), b"enabled")
            .map_err(std::io::Error::from)
    })
    .ok()
    .flatten()
    .map(|v| v == b"1")
    .unwrap_or(false);
    if !enabled {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"valid": false, "reason": "totp_not_enabled"})),
        )
            .into_response();
    }
    let code_valid = crate::handlers::totp_util::verify_code(&secret, &req.code);
    let recovery_valid = if !code_valid {
        // one-shot: a code that matches is removed in the same call
        let rc_key = key.clone();
        let code_owned = req.code.clone();
        crate::handlers::kevy_util::with_kevy(move |c| {
            crate::handlers::totp_util::consume_recovery_code(c, &rc_key, &code_owned)
        })
        .unwrap_or(false)
    } else {
        false
    };
    if code_valid || recovery_valid {
        (StatusCode::OK, Json(serde_json::json!({"valid": true}))).into_response()
    } else {
        tracing::warn!(event = "auth", channel = "verify", %caller, %source, account = %mailrs_auth_guard::login::loggable_account(&req.address), result = "mfa-failed");
        guard.failed(source, &req.address).await;
        (
            StatusCode::OK,
            Json(serde_json::json!({"valid": false, "reason": "invalid_code"})),
        )
            .into_response()
    }
}

/// POST /api/auth/change-password — verify the current password
/// against the account's stored argon2 hash, then re-hash the new
/// password and patch the account blob. Requires an authenticated
/// session (address comes from the middleware).
pub async fn change_password(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(address)): Extension<AuthedUser>,
    Json(req): Json<ChangePasswordRequest>,
) -> Response {
    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng};
    if req.current_password.is_empty() || req.new_password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "current and new password are required"})),
        )
            .into_response();
    }
    if !super::password::acceptable_new(&req.new_password) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "new password must be 8 to 1024 characters"})),
        )
            .into_response();
    }

    // Verify current password via fastcore RPC (same shape as login).
    let acct = match state.core.get_account_with_hash(&address).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(err = %e, "change_password: get_account_with_hash failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let hash = match acct.password_hash.as_deref() {
        Some(h) if !h.is_empty() => h,
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if Argon2::default()
        .verify_password(req.current_password.as_bytes(), &parsed)
        .is_err()
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "current password is incorrect"})),
        )
            .into_response();
    }

    // Hash new password.
    let salt = SaltString::generate(&mut ArgonRng);
    let new_hash = match Argon2::default().hash_password(req.new_password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Delegate the hash write to fastcore, which owns the embedded
    // kevy that login reads from. Writing to network kevy here (the
    // old code) landed on a store fastcore never consults.
    let req = mailrs_core_api::method::admin::SetPasswordRequest {
        password_hash: new_hash,
    };
    if let Err(e) = state.core.set_account_password(&address, &req).await {
        tracing::warn!(err = %e, %address, "set_account_password failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Revoke all outstanding sessions for this address — the address
    // itself owns the session:by_addr:<address> set of tokens.
    let addr_c = address.clone();
    let _ = crate::handlers::kevy_util::with_kevy(move |c| super::revoke_sessions(c, &addr_c));
    (StatusCode::OK, Json(serde_json::json!({"success": true}))).into_response()
}
