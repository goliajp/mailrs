//! Clearing passwords: a super administrator empties the passwords of
//! accounts whose credentials may have leaked, so that no password signs in
//! until each owner sets a new one with a code they are handed in person.
//!
//! The preview shows what clearing would cut off, so an account that runs a
//! service (agent keys, SMTP sign-ins with its password) is recognised
//! before it is cleared. Nothing is left out on the operator's behalf.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use mailrs_auth_guard::events::{Channel, Outcome, Query};
use mailrs_core_api::method::admin::ClearPasswordRequest;
use serde::{Deserialize, Serialize};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// Most accounts one request takes.
pub const MAX_ACCOUNTS: usize = 100;

#[derive(Debug, Deserialize)]
pub struct Addresses {
    pub addresses: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentKey {
    pub id: serde_json::Value,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
}

/// One account as clearing would find it.
#[derive(Debug, Serialize)]
pub struct PreviewRow {
    pub address: String,
    pub exists: bool,
    /// Already cleared, and when; `null` when it was not.
    pub cleared_at: Option<i64>,
    /// Web and app sessions that clearing ends.
    pub sessions: usize,
    /// Agent keys keep working after clearing: listed so the operator sees
    /// them.
    pub agent_keys: Vec<AgentKey>,
    pub totp: bool,
    pub external_logins: usize,
    /// Successful SMTP sign-ins with the password in the last seven days: a
    /// device or a service that stops sending once it is cleared.
    pub smtp_password_logins_7d: usize,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub items: Vec<PreviewRow>,
    /// Whether anyone may hand out reset codes. Without one, a cleared
    /// account has no way back in, and the page says so before clearing.
    pub issuers_configured: bool,
}

#[derive(Debug, Serialize)]
pub struct Cleared {
    pub address: String,
    /// `cleared`, or `unknown` for an address with no account.
    pub result: &'static str,
    pub sessions_revoked: usize,
}

/// The addresses, trimmed, lowercased and once each, at most
/// [`MAX_ACCOUNTS`] of them.
fn addresses(req: Addresses) -> Result<Vec<String>, StatusCode> {
    let mut out: Vec<String> = Vec::new();
    for a in req.addresses {
        let a = a.trim().to_ascii_lowercase();
        if !a.is_empty() && !out.contains(&a) {
            out.push(a);
        }
    }
    match out.len() {
        0 => Err(StatusCode::BAD_REQUEST),
        n if n > MAX_ACCOUNTS => Err(StatusCode::PAYLOAD_TOO_LARGE),
        _ => Ok(out),
    }
}

/// Who may hand out reset codes, from `MAILRS_RESET_ISSUERS`.
pub(crate) fn issuers() -> Vec<String> {
    std::env::var("MAILRS_RESET_ISSUERS")
        .unwrap_or_default()
        .split(',')
        .map(|a| a.trim().to_ascii_lowercase())
        .filter(|a| !a.is_empty())
        .collect()
}

/// POST /api/admin/password-clear/preview — super administrators only (the
/// admin middleware), with a recent confirmation.
pub async fn preview(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Json(req): Json<Addresses>,
) -> Result<Json<Preview>, StatusCode> {
    let mut items = Vec::new();
    for address in addresses(req)? {
        items.push(row(&state, address).await?);
    }
    Ok(Json(Preview {
        items,
        issuers_configured: !issuers().is_empty(),
    }))
}

async fn row(state: &WebState, address: String) -> Result<PreviewRow, StatusCode> {
    let account = match state.core.get_account_with_hash(&address).await {
        Ok(a) => Some(a),
        Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => None,
        Err(e) => return Err(crate::handlers::admin::map_err(e)),
    };
    let Some(account) = account else {
        return Ok(PreviewRow {
            address,
            exists: false,
            cleared_at: None,
            sessions: 0,
            agent_keys: Vec::new(),
            totp: false,
            external_logins: 0,
            smtp_password_logins_7d: 0,
        });
    };
    let addr = address.clone();
    let (sessions, keys, totp, links) = with_kevy(move |c| {
        let sessions =
            c.scard(mailrs_core_sidestate::keys::session::by_addr_key(&addr).as_bytes())?;
        let keys = crate::handlers::complete::hgetall_values(c, &format!("agent:keys:{addr}"))?;
        let totp = c.hget(format!("totp:{addr}").as_bytes(), b"enabled")?;
        let links = mailrs_core_sidestate::families::identity_link::links_for(c, &addr)?;
        Ok((sessions, keys, totp, links))
    })?;
    let since = crate::handlers::complete::now_secs().max(0) as u64 - 7 * 86_400;
    let smtp = crate::login_guard::GUARD
        .events(Query {
            account: Some(address.clone()),
            channel: Some(Channel::Smtp),
            result: Some(Outcome::Ok),
            since: Some(since),
            limit: Some(mailrs_auth_guard::events::MAX_LIMIT),
            ..Query::default()
        })
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "password clear preview: sign-in events unreachable");
            StatusCode::SERVICE_UNAVAILABLE
        })?;
    Ok(PreviewRow {
        address,
        exists: true,
        cleared_at: account.public.password_cleared_at,
        sessions,
        agent_keys: keys
            .iter()
            .filter_map(|k| serde_json::from_slice::<serde_json::Value>(k).ok())
            .map(|k| AgentKey {
                id: k["id"].clone(),
                name: k["name"].as_str().unwrap_or_default().to_string(),
                prefix: k["prefix"].as_str().unwrap_or_default().to_string(),
                scopes: serde_json::from_value(k["scopes"].clone()).unwrap_or_default(),
            })
            .collect(),
        totp: totp.as_deref() == Some(b"1"),
        external_logins: links.len(),
        smtp_password_logins_7d: smtp.len(),
    })
}

/// POST /api/admin/password-clear — clear each account's password and end
/// its sessions. Agent keys stay: they are revoked by their own route.
pub async fn clear(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<Addresses>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut items = Vec::new();
    for address in addresses(req)? {
        let by = ClearPasswordRequest { by: actor.clone() };
        match state.core.clear_account_password(&address, &by).await {
            Ok(()) => {}
            Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => {
                items.push(Cleared {
                    address,
                    result: "unknown",
                    sessions_revoked: 0,
                });
                continue;
            }
            Err(e) => return Err(crate::handlers::admin::map_err(e)),
        }
        let addr = address.clone();
        let sessions = with_kevy(move |c| crate::handlers::auth::revoke_sessions(c, &addr))?;
        crate::handlers::audit::record(
            &actor,
            "account.password_clear",
            &address,
            &format!("sessions_revoked={sessions}"),
        );
        items.push(Cleared {
            address,
            result: "cleared",
            sessions_revoked: sessions,
        });
    }
    Ok(Json(serde_json::json!({ "items": items })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(a: &[&str]) -> Addresses {
        Addresses {
            addresses: a.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn addresses_are_trimmed_lowercased_and_taken_once() {
        assert_eq!(
            addresses(req(&[" A@x.test ", "a@x.test", "", "b@x.test"])).unwrap(),
            vec!["a@x.test", "b@x.test"]
        );
    }

    #[test]
    fn none_or_too_many_are_refused() {
        assert_eq!(addresses(req(&["", " "])), Err(StatusCode::BAD_REQUEST));
        let many: Vec<String> = (0..=MAX_ACCOUNTS).map(|i| format!("u{i}@x.test")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(addresses(req(&many)), Err(StatusCode::PAYLOAD_TOO_LARGE));
    }
}
