//! Reset codes: an issuer (`MAILRS_RESET_ISSUERS`) hands the owner of an
//! account whose password was cleared a one-time code, in person, and the
//! owner sets a new password with it.
//!
//! The network kevy holds only an HMAC of the code under
//! `MAILRS_RESET_CODE_KEY`, a key that lives in the server's configuration
//! and nowhere in kevy: reading the store gives no code, and a code from
//! before the key was changed no longer matches.
//!
//! The route sits outside `/api/admin/`: an issuer need not be an
//! administrator.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::WebState;
use crate::handlers::complete::now_secs;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// No `0 O 1 I`: a code read aloud or copied from a screen is typed again.
const ALPHABET: &[u8; 32] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
/// 10 characters of 32: 50 bits, against an attacker who gets five tries.
const LENGTH: usize = 10;
/// How long a code works, unless `MAILRS_RESET_CODE_TTL_HOURS` says.
const DEFAULT_TTL_HOURS: u64 = 24;

/// A new code.
fn new_code() -> String {
    let mut bytes = [0u8; LENGTH];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    bytes
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

/// A code as typed: spaces and dashes dropped, letters raised.
pub(crate) fn normalized(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The key from `MAILRS_RESET_CODE_KEY`: hex, at least 32 bytes. `None`
/// when it is missing or too short, and no code can be issued or used.
pub(crate) fn key() -> Option<Vec<u8>> {
    let raw = std::env::var("MAILRS_RESET_CODE_KEY").ok()?;
    hex::decode(raw.trim()).ok().filter(|k| k.len() >= 32)
}

/// Which key made a code: changing the key makes every code made before
/// it stop matching, and the status page says so.
pub(crate) fn key_id(key: &[u8]) -> String {
    hex::encode(&Sha256::digest(key)[..4])
}

/// The code's MAC, bound to the account, so one account's code cannot be
/// used on another.
fn hmac_of(key: &[u8], address: &str, code: &str) -> Hmac<Sha256> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac takes any key length");
    m.update(address.to_ascii_lowercase().as_bytes());
    m.update(b"\n");
    m.update(normalized(code).as_bytes());
    m
}

pub(crate) fn mac(key: &[u8], address: &str, code: &str) -> String {
    hex::encode(hmac_of(key, address, code).finalize().into_bytes())
}

/// Whether `mac_hex` is the MAC of `code`, compared in constant time.
pub(crate) fn matches(key: &[u8], address: &str, code: &str, mac_hex: &str) -> bool {
    let Ok(want) = hex::decode(mac_hex) else {
        return false;
    };
    hmac_of(key, address, code).verify_slice(&want).is_ok()
}

fn ttl_secs() -> u64 {
    std::env::var("MAILRS_RESET_CODE_TTL_HOURS")
        .ok()
        .and_then(|h| h.trim().parse::<u64>().ok())
        .filter(|h| *h > 0)
        .unwrap_or(DEFAULT_TTL_HOURS)
        * 3600
}

fn refused(status: StatusCode, error: &str) -> Response {
    (status, Json(serde_json::json!({ "error": error }))).into_response()
}

fn is_issuer(actor: &str) -> bool {
    crate::handlers::admin_password_clear::issuers().contains(&actor.to_ascii_lowercase())
}

/// POST /api/reset-codes/{address} — issue a code for an account whose
/// password was cleared. The code is in this answer and nowhere else.
pub async fn issue(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Response, StatusCode> {
    if !is_issuer(&actor) {
        return Ok(refused(StatusCode::FORBIDDEN, "not_an_issuer"));
    }
    let Some(key) = key() else {
        // not 503: that is the server restarting, which this is not
        return Ok(refused(
            StatusCode::NOT_IMPLEMENTED,
            "reset_codes_not_configured",
        ));
    };
    let address = address.trim().to_ascii_lowercase();
    let account = match state.core.get_account_with_hash(&address).await {
        Ok(a) => a,
        Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => {
            return Ok(refused(StatusCode::NOT_FOUND, "no_such_account"));
        }
        Err(e) => return Err(crate::handlers::admin::map_err(e)),
    };
    if account.public.password_cleared_at.is_none() {
        return Ok(refused(StatusCode::CONFLICT, "password_not_cleared"));
    }
    let code = new_code();
    let now = now_secs();
    let ttl = ttl_secs();
    let expires_at = now + ttl as i64;
    let row = [
        ("mac", mac(&key, &address, &code)),
        ("key_id", key_id(&key)),
        ("issued_by", actor.clone()),
        ("issued_at", now.to_string()),
        ("expires_at", expires_at.to_string()),
    ];
    let k = mailrs_core_sidestate::keys::reset::code_key(&address);
    let f = mailrs_core_sidestate::keys::reset::failures_key(&address);
    with_kevy(move |c| {
        // replacing the hash is what makes an earlier code stop working
        c.del(&[k.as_bytes(), f.as_bytes()])?;
        let fields: Vec<(&[u8], &[u8])> = row
            .iter()
            .map(|(f, v)| (f.as_bytes(), v.as_bytes()))
            .collect();
        c.hset(k.as_bytes(), &fields)?;
        c.expire(k.as_bytes(), std::time::Duration::from_secs(ttl))?;
        Ok(())
    })?;
    crate::handlers::audit::record(
        &actor,
        "reset_code.issue",
        &address,
        &format!("expires_at={expires_at}"),
    );
    let alert = mailrs_auth_guard::alerts::Alert {
        kind: mailrs_auth_guard::alerts::Kind::ResetCodeIssued,
        subject: address.clone(),
        at: now as u64,
        detail: format!("issued by {actor}, valid until {expires_at} (unix seconds)"),
        facts: vec![
            mailrs_auth_guard::alerts::Fact::IssuedBy {
                actor: actor.clone(),
            },
            mailrs_auth_guard::alerts::Fact::ValidUntil {
                at: expires_at.max(0) as u64,
            },
        ],
    };
    if let Err(e) = crate::login_guard::GUARD.raise(alert).await {
        tracing::warn!(error = %e, "reset code issued, but the alert could not be queued");
    }
    Ok(Json(serde_json::json!({ "code": code, "expires_at": expires_at })).into_response())
}

/// GET /api/reset-codes/{address} — whether a code is out for the account,
/// until when, and how many wrong tries it has had; never the code.
pub async fn status(
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Response, StatusCode> {
    if !is_issuer(&actor) {
        return Ok(refused(StatusCode::FORBIDDEN, "not_an_issuer"));
    }
    let k = mailrs_core_sidestate::keys::reset::code_key(address.trim());
    let f = mailrs_core_sidestate::keys::reset::failures_key(address.trim());
    let row = with_kevy(move |c| {
        let get = |c: &mut kevy_client::Connection, f: &[u8]| -> std::io::Result<Option<String>> {
            Ok(c.hget(k.as_bytes(), f)?
                .and_then(|v| String::from_utf8(v).ok()))
        };
        Ok((
            get(c, b"issued_by")?,
            get(c, b"issued_at")?,
            get(c, b"expires_at")?,
            c.get(f.as_bytes())?.and_then(|v| String::from_utf8(v).ok()),
            get(c, b"key_id")?,
        ))
    })?;
    let (Some(issued_by), Some(issued_at), Some(expires_at), failures, made_by) = row else {
        return Ok(Json(serde_json::json!({ "issued": false })).into_response());
    };
    let current = key().map(|k| key_id(&k));
    Ok(Json(serde_json::json!({
        "issued": true,
        "issued_by": issued_by,
        "issued_at": issued_at.parse::<i64>().ok(),
        "expires_at": expires_at.parse::<i64>().ok(),
        "failures": failures.and_then(|f| f.parse::<u32>().ok()).unwrap_or(0),
        // false after the key was changed: the code no longer works
        "key_current": made_by.is_some() && made_by == current,
    }))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7u8; 32];

    #[test]
    fn a_code_is_ten_characters_nobody_misreads() {
        for _ in 0..200 {
            let code = new_code();
            assert_eq!(code.len(), LENGTH);
            assert!(code.bytes().all(|b| ALPHABET.contains(&b)), "{code}");
            assert!(!code.contains(['0', 'O', '1', 'I']), "{code}");
        }
    }

    #[test]
    fn a_code_matches_its_own_mac_and_account_only() {
        let m = mac(&KEY, "Alice@x.test", "ABCDE23456");
        assert!(matches(&KEY, "alice@x.test", "ABCDE23456", &m));
        // typed with a dash, a space and in lower case
        assert!(matches(&KEY, "alice@x.test", "abcde-234 56", &m));
        assert!(
            !matches(&KEY, "bob@x.test", "ABCDE23456", &m),
            "another account"
        );
        assert!(
            !matches(&KEY, "alice@x.test", "ABCDE23457", &m),
            "another code"
        );
        assert!(
            !matches(&[8u8; 32], "alice@x.test", "ABCDE23456", &m),
            "another key"
        );
        assert!(!matches(&KEY, "alice@x.test", "ABCDE23456", "not hex"));
    }

    #[test]
    fn a_changed_key_is_told_apart() {
        assert_ne!(key_id(&KEY), key_id(&[8u8; 32]));
        assert_eq!(key_id(&KEY).len(), 8);
    }
}
