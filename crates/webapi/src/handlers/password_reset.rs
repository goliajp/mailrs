//! Setting a new password with a reset code, on an account whose password
//! an administrator cleared. No session is needed and none is issued: the
//! owner signs in afterwards as usual, with a second factor if enrolled.
//!
//! Every way of getting it wrong (no such account, an account that was not
//! cleared, no code out, a code from before the key changed, a wrong code)
//! gets the same answer and counts against the login guard, as a wrong
//! password does. Five wrong codes and the code is gone.

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use mailrs_auth_guard::events::{Channel, Credential, LoginEvent, Outcome};
use mailrs_auth_guard::login::Verdict;

use crate::WebState;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::reset_code;
use crate::login_guard::ClientIp;

/// Wrong codes a code survives.
const MAX_FAILURES: u32 = 5;

#[derive(Debug, serde::Deserialize)]
pub struct ResetRequest {
    pub address: String,
    pub code: String,
    pub new_password: String,
}

fn invalid() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": "invalid_reset_code" })),
    )
        .into_response()
}

/// The code out for `address`: (mac, key_id), when there is one.
fn code_row(address: &str) -> Result<Option<(String, String)>, StatusCode> {
    let k = mailrs_core_sidestate::keys::reset::code_key(address);
    let (mac, key_id) = with_kevy(move |c| {
        Ok((
            c.hget(k.as_bytes(), b"mac")?,
            c.hget(k.as_bytes(), b"key_id")?,
        ))
    })?;
    let text = |v: Option<Vec<u8>>| v.and_then(|b| String::from_utf8(b).ok());
    Ok(match (text(mac), text(key_id)) {
        (Some(mac), Some(key_id)) => Some((mac, key_id)),
        _ => None,
    })
}

/// POST /api/auth/password-reset
pub async fn reset(
    State(state): State<Arc<WebState>>,
    ClientIp(source): ClientIp,
    headers: axum::http::HeaderMap,
    Json(req): Json<ResetRequest>,
) -> Response {
    if let Some(refused) =
        crate::session::csrf::refuse_foreign_origin(&headers, "/api/auth/password-reset")
    {
        return refused;
    }
    let address = req.address.trim().to_ascii_lowercase();
    let guard = &*crate::login_guard::GUARD;
    let event = |result| {
        LoginEvent::new(
            Channel::Web,
            Credential::ResetCode,
            source,
            &address,
            result,
        )
    };
    let verdict = guard.check(source, &address).await;
    if let Verdict::Locked { .. } = verdict {
        guard.record(event(Outcome::Locked)).await;
        return crate::login_guard::locked_response(verdict);
    }
    let Some(key) = reset_code::key() else {
        // not 503: that is the server restarting, which this is not
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({ "error": "reset_codes_not_configured" })),
        )
            .into_response();
    };
    let cleared = match state.core.get_account_with_hash(&address).await {
        Ok(a) => a.public.password_cleared_at.is_some(),
        Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => false,
        Err(e) => return crate::handlers::admin::map_err(e).into_response(),
    };
    let row = match code_row(&address) {
        Ok(r) => r,
        Err(status) => return status.into_response(),
    };
    let right = match (&row, cleared) {
        (Some((mac, key_id)), true) => {
            *key_id == reset_code::key_id(&key)
                && reset_code::matches(&key, &address, &req.code, mac)
        }
        _ => false,
    };
    if !right {
        guard.failed(source, &address).await;
        guard.record(event(Outcome::BadPassword)).await;
        if row.is_some() {
            count_failure(&address, source);
        }
        return invalid();
    }
    // a refused password leaves the code as it was, for another try
    if let Err(weak) = mailrs_auth_guard::password::check_new(&req.new_password, &address) {
        return crate::handlers::auth::password::refused(weak);
    }
    let hash = {
        use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
        let salt = SaltString::generate(&mut OsRng);
        match argon2::Argon2::default().hash_password(req.new_password.as_bytes(), &salt) {
            Ok(h) => h.to_string(),
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };
    // setting the hash also removes the record that the password was cleared
    let set = mailrs_core_api::method::admin::SetPasswordRequest {
        password_hash: hash,
    };
    if let Err(e) = state.core.set_account_password(&address, &set).await {
        return crate::handlers::admin::map_err(e).into_response();
    }
    let k = mailrs_core_sidestate::keys::reset::code_key(&address);
    let f = mailrs_core_sidestate::keys::reset::failures_key(&address);
    let addr = address.clone();
    if let Err(status) = with_kevy(move |c| {
        c.del(&[k.as_bytes(), f.as_bytes()])?;
        crate::handlers::auth::revoke_sessions(c, &addr)?;
        Ok(())
    }) {
        return status.into_response();
    }
    guard.succeeded(source, &address).await;
    guard.record(event(Outcome::Ok)).await;
    crate::handlers::audit::record(
        &address,
        "account.password_set_by_code",
        &address,
        &format!("source={source}"),
    );
    StatusCode::NO_CONTENT.into_response()
}

/// Count a wrong code against the code itself; the fifth ends it.
fn count_failure(address: &str, source: std::net::IpAddr) {
    let k = mailrs_core_sidestate::keys::reset::code_key(address);
    let f = mailrs_core_sidestate::keys::reset::failures_key(address);
    let counted = with_kevy(move |c| {
        let n = c.incr(f.as_bytes())?;
        if n == 1 {
            // gone no later than the code it counts against
            let until = c
                .hget(k.as_bytes(), b"expires_at")?
                .and_then(|v| String::from_utf8(v).ok())
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let left = (until - crate::handlers::complete::now_secs()).max(1) as u64;
            c.expire(f.as_bytes(), std::time::Duration::from_secs(left))?;
        }
        if n >= i64::from(MAX_FAILURES) {
            c.del(&[k.as_bytes(), f.as_bytes()])?;
        }
        Ok(n)
    });
    let Ok(n) = counted else {
        tracing::warn!("reset code: a wrong try could not be counted");
        return;
    };
    crate::handlers::audit::record(
        "",
        "reset_code.failed",
        address,
        &format!("source={source} try={n}"),
    );
    if n >= i64::from(MAX_FAILURES) {
        crate::handlers::audit::record("", "reset_code.expired_by_failures", address, "");
    }
}
