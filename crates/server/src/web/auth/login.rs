//! Login + session lifecycle: login_inner orchestrator,
//! verify-credentials, verify-totp, logout, auth_me.

#![allow(unused_imports)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Json, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::inbound::auth_guard::AuthCheck;

use super::super::{AuthUser, SessionInfo, WebState};
use super::*;

pub(crate) async fn login(
    state: State<Arc<WebState>>,
    addr: ConnectInfo<SocketAddr>,
    req: Json<LoginRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let resp = login_inner(state, addr, req).await.into_response();
    // if login succeeded (200), extract token from body and set as cookie
    // so OIDC authorize redirect (browser navigation) can find the session
    if resp.status() == StatusCode::OK {
        // read token from response body
        let (parts, body) = resp.into_parts();
        let bytes = axum::body::to_bytes(body, 4096).await.unwrap_or_default();
        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && let Some(token) = val["token"].as_str() {
                let cookie = format!(
                    "mailrs_session={token}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=3600"
                );
                let mut parts = parts;
                parts.headers.insert(
                    axum::http::header::SET_COOKIE,
                    cookie.parse().unwrap(),
                );
                return axum::response::Response::from_parts(parts, axum::body::Body::from(bytes));
            }
        return axum::response::Response::from_parts(parts, axum::body::Body::from(bytes));
    }
    resp
}

async fn login_inner(
    State(state): State<Arc<WebState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    if let Some(resp) = validate_login_input_lengths(&req) {
        return resp;
    }
    if state.domain_store.is_none() {
        return auth_not_configured();
    }
    if let Some(resp) = lockout_response(&state, addr.ip(), &req.address) {
        return resp;
    }

    let account = match verify_password_and_load_account(&state, &req, addr.ip()).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    match check_totp(&state, &req, addr.ip()).await {
        TotpOutcome::Ok => {}
        TotpOutcome::RequiresCode => return totp_required_response(),
        TotpOutcome::Failed(resp) => return resp,
    }

    issue_session_response(&state, &account, addr.ip()).await
}

fn validate_login_input_lengths(
    req: &LoginRequest,
) -> Option<(StatusCode, Json<serde_json::Value>)> {
    if req.address.len() > crate::web::MAX_ADMIN_FIELD_LEN || req.password.len() > 1024 {
        Some((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid input length"})),
        ))
    } else {
        None
    }
}

fn auth_not_configured() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "auth not configured"})),
    )
}

/// If `state.auth_guard` reports a lockout for `(ip, address)`,
/// produce the 429 response. Otherwise return None and the caller
/// proceeds to credential verification.
fn lockout_response(
    state: &WebState,
    ip: std::net::IpAddr,
    address: &str,
) -> Option<(StatusCode, Json<serde_json::Value>)> {
    let guard = state.auth_guard.as_ref()?;
    if let AuthCheck::LockedOut { remaining_secs } = guard.check(ip, address) {
        Some((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": format!("Too many auth failures, try again in {remaining_secs}s")
            })),
        ))
    } else {
        None
    }
}

/// Look up the account, verify the password (argon2 OR plain
/// fallback), try LDAP if password failed. Records failures on
/// the auth_guard, logs to audit. Returns the account on success
/// or the 401 response on any failure.
async fn verify_password_and_load_account(
    state: &Arc<WebState>,
    req: &LoginRequest,
    ip: std::net::IpAddr,
) -> Result<crate::domain_store::Account, (StatusCode, Json<serde_json::Value>)> {
    let ds = state.domain_store.as_ref().expect("checked in caller");

    let (account, password_hash) = match ds.get_account_with_hash(&req.address).await {
        Ok(Some(pair)) => pair,
        _ => {
            // constant-time rejection: spend the same time as a real argon2 verify
            crate::users::dummy_verify(&req.password);
            if let Some(ref guard) = state.auth_guard {
                guard.record_failure(ip, &req.address);
            }
            state
                .auth_failure_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            metrics::counter!("mailrs_auth_total", "outcome" => "failure").increment(1);
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid credentials"})),
            ));
        }
    };

    if !account.active {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "account disabled"})),
        ));
    }

    let mut valid = if password_hash.is_empty() {
        false
    } else if password_hash.starts_with("$argon2") {
        crate::users::UserStore::verify_hash(&req.password, &password_hash)
    } else {
        password_hash == req.password
    };

    if !valid
        && let Some(ref ldap) = state.ldap_config
    {
        valid = ldap.authenticate(&req.address, &req.password).await;
    }

    if !valid {
        if let Some(ref guard) = state.auth_guard {
            guard.record_failure(ip, &req.address);
        }
        ds.log_audit(&req.address, "login_failed", "", &format!("ip={ip}"))
            .await;
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"})),
        ));
    }

    Ok(account)
}

/// Check TOTP 2FA per RFC 6238. Returns `Ok` if 2FA is disabled
/// or the code (or one-shot recovery code) verifies. Records
/// failures on the auth_guard + writes a `totp_failed` audit row;
/// records a `recovery_code_used` audit row when the user falls
/// back to a recovery code.
async fn check_totp(
    state: &Arc<WebState>,
    req: &LoginRequest,
    ip: std::net::IpAddr,
) -> TotpOutcome {
    let Some(ref ds) = state.domain_store else {
        return TotpOutcome::Ok;
    };
    let Ok(Some((secret, true, _recovery_codes))) = ds.get_totp_secret(&req.address).await else {
        return TotpOutcome::Ok;
    };
    let Some(code) = req.totp_code.as_ref() else {
        return TotpOutcome::RequiresCode;
    };

    let code_valid = crate::totp::verify_code(&secret, code);
    let recovery_valid = if !code_valid {
        ds.consume_recovery_code(&req.address, code)
            .await
            .unwrap_or(false)
    } else {
        false
    };

    if !code_valid && !recovery_valid {
        if let Some(ref guard) = state.auth_guard {
            guard.record_failure(ip, &req.address);
        }
        ds.log_audit(&req.address, "totp_failed", "", &format!("ip={ip}"))
            .await;
        return TotpOutcome::Failed((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid TOTP code"})),
        ));
    }

    if recovery_valid {
        ds.log_audit(
            &req.address,
            "recovery_code_used",
            "",
            &format!("ip={ip}"),
        )
        .await;
    }
    TotpOutcome::Ok
}

fn totp_required_response() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({"requires_totp": true})),
    )
}

/// Record success on auth_guard + metrics + audit log, load
/// effective permissions, generate a session token, return the
/// 200 login response with the token + display name + perms.
async fn issue_session_response(
    state: &Arc<WebState>,
    account: &crate::domain_store::Account,
    ip: std::net::IpAddr,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(ref guard) = state.auth_guard {
        guard.record_success(ip, &account.address);
    }
    state
        .auth_success_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    metrics::counter!("mailrs_auth_total", "outcome" => "success").increment(1);

    if let Some(ref ds) = state.domain_store {
        ds.log_audit(&account.address, "login", "", &format!("ip={ip}"))
            .await;
    }

    let permissions = if let Some(ref ds) = state.domain_store {
        Arc::new(
            ds.load_account_permissions(&account.address)
                .await
                .unwrap_or_else(|_| {
                    crate::permission::compute_effective_permissions(&[], &[], &[])
                }),
        )
    } else {
        Arc::new(crate::permission::compute_effective_permissions(
            &[], &[], &[],
        ))
    };

    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    state.sessions.insert(
        token.clone(),
        SessionInfo {
            address: account.address.clone(),
            display_name: account.display_name.clone(),
            permissions: permissions.clone(),
            created_at: std::time::Instant::now(),
        },
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "token": token,
            "address": account.address,
            "display_name": account.display_name,
            "permissions": permissions.permission_list(),
            "accessible_domains": permissions.accessible_domains(),
            "send_as": permissions.send_as(),
        })),
    )
}

/// verify a user's password without creating a session.
/// requires `internal.rpc` permission (only for trusted internal services).
pub(crate) async fn verify_credentials(
    AuthUser { permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<VerifyRequest>,
) -> impl IntoResponse {
    if !permissions.has("internal.rpc") {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "permission denied"})),
        );
    }

    if req.address.len() > crate::web::MAX_ADMIN_FIELD_LEN || req.password.len() > 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid input length"})),
        );
    }

    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth not configured"})),
        );
    };

    let (account, password_hash) = match ds.get_account_with_hash(&req.address).await {
        Ok(Some(pair)) => pair,
        _ => {
            crate::users::dummy_verify(&req.password);
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "valid": false,
                    "reason": "account_not_found"
                })),
            );
        }
    };

    if !account.active {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "valid": false,
                "reason": "account_disabled"
            })),
        );
    }

    // verify password
    let mut valid = if password_hash.is_empty() {
        false
    } else if password_hash.starts_with("$argon2") {
        crate::users::UserStore::verify_hash(&req.password, &password_hash)
    } else {
        password_hash == req.password
    };

    // LDAP fallback
    if !valid
        && let Some(ref ldap) = state.ldap_config {
            valid = ldap.authenticate(&req.address, &req.password).await;
        }

    if !valid {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "valid": false,
                "reason": "invalid_password"
            })),
        );
    }

    // check if TOTP is enabled
    let totp_required = matches!(ds.get_totp_secret(&req.address).await, Ok(Some((_, true, _))));

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "valid": true,
            "display_name": account.display_name,
            "domain": account.domain,
            "totp_required": totp_required
        })),
    )
}

/// verify a TOTP code for a user.
/// requires `internal.rpc` permission.
pub(crate) async fn verify_totp(
    AuthUser { permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<VerifyTotpRequest>,
) -> impl IntoResponse {
    if !permissions.has("internal.rpc") {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "permission denied"})),
        );
    }

    if req.address.len() > crate::web::MAX_ADMIN_FIELD_LEN || req.code.len() > 32 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid input length"})),
        );
    }

    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth not configured"})),
        );
    };

    let (secret, enabled, _codes) = match ds.get_totp_secret(&req.address).await {
        Ok(Some(s)) => s,
        _ => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({"valid": false, "reason": "totp_not_configured"})),
            );
        }
    };

    if !enabled {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"valid": false, "reason": "totp_not_enabled"})),
        );
    }

    let code_valid = crate::totp::verify_code(&secret, &req.code);
    let recovery_valid = if !code_valid {
        ds.consume_recovery_code(&req.address, &req.code).await.unwrap_or(false)
    } else {
        false
    };

    if code_valid || recovery_valid {
        (
            StatusCode::OK,
            Json(serde_json::json!({"valid": true})),
        )
    } else {
        (
            StatusCode::OK,
            Json(serde_json::json!({"valid": false, "reason": "invalid_code"})),
        )
    }
}

pub(crate) async fn logout(
    State(state): State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        state.sessions.remove(token);
    }
    Json(ApiResult {
        success: true,
        message: None,
    })
}

pub(crate) async fn auth_me(
    AuthUser { address, display_name, permissions, .. }: AuthUser,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "address": address,
        "display_name": display_name,
        "permissions": permissions.permission_list(),
        "accessible_domains": permissions.accessible_domains(),
        "send_as": permissions.send_as(),
    }))
}
