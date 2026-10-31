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
use serde::Deserialize;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::login_guard::ClientIp;
use mailrs_auth_guard::events::{Channel, Credential, LoginEvent, Outcome};
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
        guard
            .record(LoginEvent::new(
                Channel::Verify,
                Credential::Password,
                source,
                &req.address,
                Outcome::Locked,
            ))
            .await;
        return crate::login_guard::locked_response(verdict);
    }
    // an unknown account and a wrong password answer alike, so the
    // endpoint cannot be used to find out which accounts exist
    let acct = match super::password::verify(&state, &req.address, &req.password).await {
        Ok(Some(acct)) => acct,
        Ok(None) => {
            guard.failed(source, &req.address).await;
            guard
                .record(LoginEvent::new(
                    Channel::Verify,
                    Credential::Password,
                    source,
                    &req.address,
                    Outcome::BadPassword,
                ))
                .await;
            return (
                StatusCode::OK,
                Json(serde_json::json!({"valid": false, "reason": "invalid_credentials"})),
            )
                .into_response();
        }
        Err(status) => return status.into_response(),
    };
    guard.succeeded(source, &req.address).await;
    guard
        .record(LoginEvent::new(
            Channel::Verify,
            Credential::Password,
            source,
            &req.address,
            Outcome::Ok,
        ))
        .await;
    let totp_key = mailrs_core_sidestate::keys::credentials::totp_key(&req.address);
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
        guard
            .record(LoginEvent::new(
                Channel::Verify,
                Credential::Totp,
                source,
                &req.address,
                Outcome::Locked,
            ))
            .await;
        return crate::login_guard::locked_response(verdict);
    }
    let key = mailrs_core_sidestate::keys::credentials::totp_key(&req.address);
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
    let credential = match (code_valid, recovery_valid) {
        (false, true) => Credential::RecoveryCode,
        _ => Credential::Totp,
    };
    if code_valid || recovery_valid {
        guard
            .record(LoginEvent::new(
                Channel::Verify,
                credential,
                source,
                &req.address,
                Outcome::Ok,
            ))
            .await;
        (StatusCode::OK, Json(serde_json::json!({"valid": true}))).into_response()
    } else {
        guard.failed(source, &req.address).await;
        guard
            .record(LoginEvent::new(
                Channel::Verify,
                credential,
                source,
                &req.address,
                Outcome::MfaFailed,
            ))
            .await;
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
    ClientIp(source): ClientIp,
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
    if let Err(weak) = mailrs_auth_guard::password::check_new(&req.new_password, &address) {
        return super::password::refused(weak);
    }

    // The current password goes through the login guard like a sign-in:
    // a stolen session must not be a way to guess it without limit.
    let guard = &*crate::login_guard::GUARD;
    let verdict = guard.check(source, &address).await;
    if let Verdict::Locked { .. } = verdict {
        guard
            .record(LoginEvent::new(
                Channel::Web,
                Credential::Password,
                source,
                &address,
                Outcome::Locked,
            ))
            .await;
        return crate::login_guard::locked_response(verdict);
    }
    match super::password::verify(&state, &address, &req.current_password).await {
        Err(status) => return status.into_response(),
        Ok(Some(_)) => guard.succeeded(source, &address).await,
        Ok(None) => {
            guard.failed(source, &address).await;
            guard
                .record(LoginEvent::new(
                    Channel::Web,
                    Credential::Password,
                    source,
                    &address,
                    Outcome::BadPassword,
                ))
                .await;
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "current password is incorrect"})),
            )
                .into_response();
        }
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
