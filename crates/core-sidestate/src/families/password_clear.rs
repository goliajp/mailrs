//! Clearing passwords: emptying the passwords of accounts whose credentials
//! may have leaked, so that no password signs in until each owner sets a
//! new one with a code they are handed in person.
//!
//! Two ways in, one procedure: a super administrator in webapi, and an
//! operator on the host through the core. Each core empties the password in
//! its own store; everything after that, and the preview, is here, so the
//! two ways cannot come to do different things.

use std::io;

use mailrs_auth_guard::events::{Channel, MAX_LIMIT, Outcome, Query};
use mailrs_auth_guard::login::LoginGuard;
use serde::{Deserialize, Serialize};

use crate::keys::credentials::{agent_keys_key, pwreset_by_addr_key, pwreset_key, totp_key};
use crate::keys::session::{by_addr_key, revoke_sessions};

/// Most accounts one request takes.
pub const MAX_ACCOUNTS: usize = 100;

/// The audit action of a clearing.
pub const AUDIT_ACTION: &str = "account.password_clear";

#[derive(Debug, Deserialize)]
pub struct Addresses {
    pub addresses: Vec<String>,
}

/// Why a list of addresses was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    None,
    TooMany,
}

/// The addresses, trimmed, lowercased and once each, at most
/// [`MAX_ACCOUNTS`] of them.
pub fn addresses(req: Addresses) -> Result<Vec<String>, Refused> {
    let mut out: Vec<String> = Vec::new();
    for a in req.addresses {
        let a = a.trim().to_ascii_lowercase();
        if !a.is_empty() && !out.contains(&a) {
            out.push(a);
        }
    }
    match out.len() {
        0 => Err(Refused::None),
        n if n > MAX_ACCOUNTS => Err(Refused::TooMany),
        _ => Ok(out),
    }
}

/// Who may hand out reset codes, from `MAILRS_RESET_ISSUERS`.
pub fn issuers() -> Vec<String> {
    std::env::var("MAILRS_RESET_ISSUERS")
        .unwrap_or_default()
        .split(',')
        .map(|a| a.trim().to_ascii_lowercase())
        .filter(|a| !a.is_empty())
        .collect()
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

impl Cleared {
    pub fn unknown(address: String) -> Self {
        Self {
            address,
            result: "unknown",
            sessions_revoked: 0,
        }
    }
}

/// An account's other ways in, as the network kevy holds them.
#[derive(Default)]
pub struct Credentials {
    pub sessions: usize,
    pub agent_keys: Vec<AgentKey>,
    pub totp: bool,
    pub external_logins: usize,
}

/// `address`'s sessions, agent keys, second factor and external logins.
pub fn credentials(c: &mut kevy_client::Connection, address: &str) -> io::Result<Credentials> {
    let sessions = c.scard(by_addr_key(address).as_bytes())?;
    // field, value, field, value
    let keys = c.hgetall(agent_keys_key(address).as_bytes())?;
    let totp = c.hget(totp_key(address).as_bytes(), b"enabled")?;
    let links = crate::families::identity_link::links_for(c, address)?;
    Ok(Credentials {
        sessions,
        agent_keys: keys
            .iter()
            .skip(1)
            .step_by(2)
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
    })
}

/// Successful SMTP sign-ins with `address`'s password in the seven days
/// before `now`.
pub async fn smtp_logins(guard: &dyn LoginGuard, address: &str, now: i64) -> io::Result<usize> {
    let since = (now.max(0) as u64).saturating_sub(7 * 86_400);
    let events = guard
        .events(Query {
            account: Some(address.to_string()),
            channel: Some(Channel::Smtp),
            result: Some(Outcome::Ok),
            since: Some(since),
            limit: Some(MAX_LIMIT),
            ..Query::default()
        })
        .await?;
    Ok(events.len())
}

/// A preview row: `account` is `None` for an address with no account, else
/// when its password was last cleared.
pub fn row(
    address: String,
    account: Option<Option<i64>>,
    creds: Credentials,
    smtp_logins_7d: usize,
) -> PreviewRow {
    match account {
        None => PreviewRow {
            address,
            exists: false,
            cleared_at: None,
            sessions: 0,
            agent_keys: Vec::new(),
            totp: false,
            external_logins: 0,
            smtp_password_logins_7d: 0,
        },
        Some(cleared_at) => PreviewRow {
            address,
            exists: true,
            cleared_at,
            sessions: creds.sessions,
            agent_keys: creds.agent_keys,
            totp: creds.totp,
            external_logins: creds.external_logins,
            smtp_password_logins_7d: smtp_logins_7d,
        },
    }
}

/// Everything after the core has emptied `address`'s password: end its
/// sessions, kill a recovery link mailed earlier, close its signed-in
/// protocol connections and record who did it. Agent keys stay: they are
/// revoked by their own route.
pub fn after_clear(
    c: &mut kevy_client::Connection,
    actor: &str,
    address: &str,
    now: i64,
) -> io::Result<Cleared> {
    let sessions = revoke_sessions(c, address)?;
    // a recovery link mailed earlier would set a password the owner did
    // not get from an issuer
    if let Some(token) = c.get(pwreset_by_addr_key(address).as_bytes())? {
        let token = String::from_utf8_lossy(&token).to_string();
        c.del(&[
            pwreset_key(&token).as_bytes(),
            pwreset_by_addr_key(address).as_bytes(),
        ])?;
    }
    // and the connections already signed in over IMAP, POP3, ManageSieve
    // and SMTP, in the processes that hold them
    crate::keys::signout::publish(c, address)?;
    crate::families::audit::append(
        c,
        actor,
        AUDIT_ACTION,
        address,
        &format!("sessions_revoked={sessions}"),
        now,
    )?;
    Ok(Cleared {
        address: address.to_string(),
        result: "cleared",
        sessions_revoked: sessions,
    })
}

/// The operator's way in: on the host, through the core-api, which only
/// the host and the services beside it can reach, behind the core secret.
pub use mailrs_core_api::method::admin::{
    PATH_OPERATOR_PASSWORD_CLEAR, PATH_OPERATOR_PASSWORD_CLEAR_PREVIEW,
};

/// Who the audit names for a clearing through the core.
pub const OPERATOR: &str = "operator:devops";

/// What a core keeps of an account's password.
pub trait PasswordStore: crate::NetKevy {
    /// `None` for no account, else when its password was last cleared.
    fn cleared_at(
        &self,
        address: &str,
    ) -> impl std::future::Future<Output = io::Result<Option<Option<i64>>>> + Send;
    /// Empty the password; false for no account.
    fn clear_password(
        &self,
        address: &str,
        by: &str,
    ) -> impl std::future::Future<Output = io::Result<bool>> + Send;
    /// The network kevy, where sign-in events are counted.
    fn net_url(&self) -> Option<String>;
}

fn refused(r: Refused) -> axum::http::StatusCode {
    match r {
        Refused::None => axum::http::StatusCode::BAD_REQUEST,
        Refused::TooMany => axum::http::StatusCode::PAYLOAD_TOO_LARGE,
    }
}

fn unavailable(e: io::Error) -> axum::http::StatusCode {
    tracing::warn!(error = %e, "password clear: store unreachable");
    axum::http::StatusCode::SERVICE_UNAVAILABLE
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `POST /v1/admin/password-clear:preview` — what clearing would cut off.
pub async fn operator_preview<S: PasswordStore>(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<S>>,
    axum::Json(req): axum::Json<Addresses>,
) -> Result<axum::Json<Preview>, axum::http::StatusCode> {
    let guard = mailrs_auth_guard::kevy::guard_for(state.net_url().as_deref());
    let mut items = Vec::new();
    for address in addresses(req).map_err(refused)? {
        let account = state.cleared_at(&address).await.map_err(unavailable)?;
        if account.is_none() {
            items.push(row(address, None, Credentials::default(), 0));
            continue;
        }
        let mut c = state
            .net_conn()
            .ok_or(axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
        let creds = credentials(&mut c, &address).map_err(unavailable)?;
        let smtp = smtp_logins(guard.as_ref(), &address, now())
            .await
            .map_err(unavailable)?;
        items.push(row(address, account, creds, smtp));
    }
    Ok(axum::Json(Preview {
        items,
        issuers_configured: !issuers().is_empty(),
    }))
}

/// `POST /v1/admin/password-clear` — clear each account's password and
/// do what the super administrator's clearing does after it.
pub async fn operator_clear<S: PasswordStore>(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<S>>,
    axum::Json(req): axum::Json<Addresses>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    let mut items = Vec::new();
    for address in addresses(req).map_err(refused)? {
        if !state
            .clear_password(&address, OPERATOR)
            .await
            .map_err(unavailable)?
        {
            items.push(Cleared::unknown(address));
            continue;
        }
        let mut c = state
            .net_conn()
            .ok_or(axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
        items.push(after_clear(&mut c, OPERATOR, &address, now()).map_err(unavailable)?);
    }
    Ok(axum::Json(serde_json::json!({ "items": items })))
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
        assert_eq!(addresses(req(&["", " "])), Err(Refused::None));
        let many: Vec<String> = (0..=MAX_ACCOUNTS).map(|i| format!("u{i}@x.test")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(addresses(req(&many)), Err(Refused::TooMany));
    }
}
