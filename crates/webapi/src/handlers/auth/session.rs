//! Logging in and out: the password check, the pending-link claim,
//! and how a session cookie is issued.

//! `/api/auth/*` REST handlers.

use std::sync::Arc;

use argon2::Argon2;
use axum::{
    Json,
    extract::State,
    http::{StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Response},
};
use password_hash::{PasswordHash, PasswordVerifier};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::WebState;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub address: String,
    pub password: String,
    #[serde(default)]
    pub totp_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub address: String,
    pub display_name: String,
    pub permissions: Vec<String>,
    /// Session token — same shape as monolith. Frontend stores this
    /// in the auth store; subsequent authenticated requests send it
    /// as `Authorization: Bearer <token>`.
    pub token: String,
}

/// POST /api/auth/login
///
/// - Resolve the account + argon2 hash via core RPC
/// - Verify password
/// - Generate 32-byte random session token (hex)
/// - Write `session:<token>` to kevy with the SessionInfoWire shape the
///   monolith uses, so either binary can read it
/// - Return 200 with `Set-Cookie: mailrs_session=<token>; HttpOnly; ...`
pub async fn login(
    State(state): State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Response {
    // Fastcore-only: kevy-backed account store is the source of truth.
    let acct = match state.core.get_account_with_hash(&req.address).await {
        Ok(a) => a,
        Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Err(e) => {
            tracing::warn!(error = %e, "login: get_account_with_hash failed");
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
        .verify_password(req.password.as_bytes(), &parsed)
        .is_err()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // TOTP gate — if enrolled, require a 6-digit code (or a
    // recovery code) on the login request; otherwise short-circuit
    // with `{ requires_totp: true }` so the UI can prompt.
    //
    // Kevy failure while checking TOTP must NOT fall through to
    // "session issued" — that would let a kevy blink bypass 2FA.
    // Fail closed with 500 instead.
    let totp_key = format!("totp:{}", req.address);
    let totp_key_r = totp_key.clone();
    let totp_enrolled_secret = match crate::handlers::kevy_util::with_kevy(move |c| {
        Ok((
            c.hget(totp_key_r.as_bytes(), b"secret")?,
            c.hget(totp_key_r.as_bytes(), b"enabled")?,
        ))
    }) {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(addr = %req.address, "login: kevy TOTP check failed; rejecting to fail-closed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if let (Some(secret_bytes), Some(en)) = totp_enrolled_secret
        && en == b"1"
    {
        let secret = String::from_utf8(secret_bytes).unwrap_or_default();
        let Some(code) = req.totp_code.as_ref() else {
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "requires_totp": true })),
            )
                .into_response();
        };
        let ok_code = crate::handlers::totp_util::verify_code(&secret, code);
        let ok_recovery = if !ok_code {
            // v2 Stage B.1: read-then-write on the SAME kevy connection
            // — halves the TCP round-trip cost and (more importantly)
            // shrinks the race window between two concurrent logins
            // consuming the same recovery code from ms (two round-trips)
            // to µs (back-to-back ops on one open conn). Full atomicity
            // would require WATCH+MULTI; deferred to a follow-up as the
            // window is now short enough that in-practice collision is
            // vanishing.
            let rc_key = totp_key.clone();
            let code_owned = code.to_string();
            crate::handlers::kevy_util::with_kevy(move |c| {
                let recovery_str = c
                    .hget(rc_key.as_bytes(), b"recovery_codes")?
                    .and_then(|v| String::from_utf8(v).ok())
                    .unwrap_or_default();
                let mut codes: Vec<&str> =
                    recovery_str.split(',').filter(|s| !s.is_empty()).collect();
                let Some(idx) = codes.iter().position(|c| *c == code_owned.as_str()) else {
                    return Ok(false);
                };
                codes.remove(idx);
                let joined = codes.join(",");
                c.hset(
                    rc_key.as_bytes(),
                    &[(b"recovery_codes" as &[u8], joined.as_bytes())],
                )?;
                Ok(true)
            })
            .unwrap_or(false)
        } else {
            false
        };
        if !ok_code && !ok_recovery {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid TOTP code" })),
            )
                .into_response();
        }
    }

    // A third-party identity waiting to be linked, if this login was
    // started from one. Claimed here — after the password and any TOTP have
    // been checked, and never before — because the whole point of the
    // prompt is that a password proves the mailbox is yours.
    //
    // The account comes from `acct`, which is what just authenticated. It is
    // never read from the request body: a caller naming somebody else's
    // address must not be able to attach their own Google account to it.
    claim_pending_link(&headers, &acct.public.address).await;
    issue_session(&state, &acct).await
}

/// Link the identity parked by an external login, if one is waiting.
///
/// Best-effort by design: failing to link must not fail the login. The user
/// typed a correct password and is entitled to their mail whether or not the
/// convenience of a linked Google account survived the round trip. A failure
/// is logged and the identity simply stays unlinked, which the next attempt
/// can redo.
async fn claim_pending_link(headers: &axum::http::HeaderMap, address: &str) {
    let Some(handle) = cookie_value(headers, "mailrs_pending_link") else {
        return;
    };
    let addr = address.to_string();
    let outcome = tokio::task::spawn_blocking(move || -> std::io::Result<Option<String>> {
        // VarError is not a KevyError, so there is no engine category here
        let url = std::env::var("MAILRS_KEVY_URL").map_err(std::io::Error::other)?;
        let mut c = kevy_client::Connection::connect(&url)?;
        use mailrs_core_sidestate::families::identity_link as link;
        // Single-use: claimed and deleted together, so a captured handle
        // cannot be replayed later against a second account.
        let Some(json) = link::claim_pending(&mut c, &handle)? else {
            return Ok(None);
        };
        let identity: serde_json::Value = serde_json::from_str(&json)?;
        let issuer = identity["issuer"].as_str().unwrap_or_default().to_string();
        let subject = identity["subject"].as_str().unwrap_or_default().to_string();
        if issuer.is_empty() || subject.is_empty() {
            return Ok(None);
        }
        let outcome = link::link(&mut c, &issuer, &subject, &addr)?;
        Ok(Some(format!("{outcome:?} {issuer} {subject}")))
    })
    .await;

    match outcome {
        Ok(Ok(Some(detail))) => {
            tracing::info!(%address, %detail, "external identity linked by password login");
            crate::handlers::audit::record(address, "auth.identity.link", &detail, "");
        }
        Ok(Ok(None)) => {}
        Ok(Err(e)) => tracing::warn!(%address, err = %e, "pending link could not be claimed"),
        Err(e) => tracing::warn!(%address, err = %e, "pending link task failed"),
    }
}

/// One cookie's value out of the header.
pub(crate) fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for cookie in raw.split(';') {
        let cookie = cookie.trim();
        if let Some(rest) = cookie.strip_prefix(name)
            && let Some(v) = rest.strip_prefix('=')
            && !v.is_empty()
        {
            return Some(v.to_string());
        }
    }
    None
}

/// Mint a session for an account that has just proved it owns itself.
///
/// Shared because there is now more than one way to prove it. A second copy
/// of this would be a second definition of what a session is — the token
/// shape, the blob the auth middleware reads, the per-user index that makes
/// "revoke everything" possible, and the cookie flags. Those drifting apart
/// is not a cosmetic problem: the middleware reads one shape, and a login
/// that writes a different one produces a session that authenticates nobody.
pub(crate) async fn issue_session(
    state: &Arc<WebState>,
    acct: &mailrs_core_api::method::admin::AccountWithHashWire,
) -> axum::response::Response {
    // Permissions for the login response — fastcore-only.
    let perms = state
        .core
        .effective_permissions(&acct.public.address)
        .await
        .ok();

    // Generate token + write to kevy in the same shape as the monolith.
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    let empty_strings: Vec<String> = Vec::new();
    let perms_obj = match perms.as_ref() {
        Some(p) => serde_json::json!({
            "permissions": p.permissions,
            "is_super": p.is_super,
            "accessible_domains": empty_strings.clone(),
            "send_as": p.send_as,
        }),
        None => serde_json::json!({
            "permissions": empty_strings.clone(),
            "is_super": false,
            "accessible_domains": empty_strings.clone(),
            "send_as": empty_strings.clone(),
        }),
    };
    let blob = serde_json::json!({
        "address": acct.public.address,
        "display_name": acct.public.display_name,
        "permissions": perms_obj,
        "created_at_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    let blob_bytes = match serde_json::to_vec(&blob) {
        Ok(b) => b,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let kevy_url = std::env::var("MAILRS_KEVY_URL").ok();
    if let Some(url) = kevy_url {
        let token_clone = token.clone();
        let addr_clone = acct.public.address.clone();
        let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let mut c = kevy_client::Connection::connect(&url)?;
            let key = format!("session:{token_clone}");
            c.set_with_ttl(
                key.as_bytes(),
                &blob_bytes,
                std::time::Duration::from_secs(7 * 24 * 3600),
            )?;
            // Per-user session index — makes it possible to revoke all
            // active sessions when the password changes.
            let idx = format!("session:by_addr:{addr_clone}");
            c.sadd(idx.as_bytes(), &[token_clone.as_bytes()])?;
            Ok(())
        })
        .await;
    } else {
        tracing::warn!("login: MAILRS_KEVY_URL unset — token NOT persisted");
    }

    let display = acct.public.display_name.clone();
    let address = acct.public.address.clone();
    crate::handlers::audit::record(&address, "auth.login", &address, "");
    let perms_vec = perms.map(|p| p.permissions).unwrap_or_default();
    let body = Json(LoginResponse {
        address,
        display_name: display,
        permissions: perms_vec,
        token: token.clone(),
    });
    let cookie =
        format!("mailrs_session={token}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=604800");

    let mut resp = body.into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&cookie) {
        resp.headers_mut().insert(SET_COOKIE, v);
    }
    resp
}

/// POST /api/auth/logout
///
/// Deletes the kevy `session:<token>` blob. The cookie is also cleared
/// via `Set-Cookie: mailrs_session=; Max-Age=0`.
pub async fn logout(req: axum::extract::Request) -> Response {
    let token = req
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|c| {
                let c = c.trim();
                c.strip_prefix("mailrs_session=").map(|s| s.to_string())
            })
        });

    if let (Some(t), Ok(url)) = (token, std::env::var("MAILRS_KEVY_URL")) {
        let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let mut c = kevy_client::Connection::connect(&url)?;
            let key = format!("session:{t}");
            let _ = c.del(&[key.as_bytes()])?;
            Ok(())
        })
        .await;
    }

    let cookie = "mailrs_session=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0";
    let mut resp = StatusCode::NO_CONTENT.into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(cookie) {
        resp.headers_mut().insert(SET_COOKIE, v);
    }
    resp
}
