//! Session resolution against the shared kevy-server.
//!
//! The `mailrs_session` blob (same shape the monolith wrote) is JSON:
//!
//! ```json
//! {
//!   "address": "user@x.com",
//!   "display_name": "User",
//!   "permissions": { ... },
//!   "created_at_unix": 1700000000
//! }
//! ```
//!
//! Key shape: `session:<token>`. TTL 7 days. The exact same shape so a
//! webapi binary can read sessions created by the existing monolith
//! during the cutover window.

use std::sync::Arc;

use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use serde::Deserialize;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

mod scopes;
use scopes::agent_scopes_allow;
pub use scopes::{
    AgentScopes, is_known_scope, mock_ns_restriction, send_only, validate_agent_scopes,
};

/// `session:<token>` blob — fields webapi reads. `permissions` is
/// already serialized by the monolith but we only need the address +
/// display_name here; permissions are re-fetched via core_client.
#[derive(Debug, Deserialize)]
struct SessionBlob {
    address: String,
    #[serde(default)]
    display_name: String,
}

/// kevy connection URL env var (matches the monolith's
/// `MAILRS_KEVY_URL`).
const KEVY_URL_ENV: &str = "MAILRS_KEVY_URL";

const SESSION_COOKIE: &str = "mailrs_session";
use mailrs_core_sidestate::keys::session::SESSION_PREFIX as SESSION_KEY_PREFIX;

/// Agent API keys (`mk_…`, created via POST /api/agent/keys) are an
/// alternative bearer credential for machine callers. The secret index
/// `agent:key:secret:<key>` stores `{"user":…,"id":…}`; the key is live
/// only while its record still exists in the `agent:keys:<user>` hash,
/// so deletion revokes immediately even though the index lingers.
pub(crate) const AGENT_KEY_PREFIX: &str = "mk_";
const AGENT_KEY_INDEX_PREFIX: &str = "agent:key:secret:";

#[derive(Debug, Deserialize)]
struct AgentKeyIndex {
    user: String,
    id: i64,
}

/// The scopes stored on the key record. Empty = full owner access
/// (user decision 2026-07-18: existing keys keep working); a non-empty
/// list is enforced — see `scopes::agent_scopes_allow`.
#[derive(Debug, Deserialize)]
struct AgentKeyRecord {
    #[serde(default)]
    scopes: Vec<String>,
}

/// An authenticated caller: owner + its (possibly empty) scope set. A
/// browser session always carries empty scopes.
pub struct AgentAuth {
    pub user: String,
    pub scopes: Vec<String>,
    /// The key's id in its owner's `agent:keys:` hash; `None` for a
    /// browser session.
    pub key_id: Option<i64>,
}

/// The agent key a request came in with, next to `AuthedUser`. Absent for
/// a browser session.
#[derive(Debug, Clone)]
pub struct AgentKey {
    pub id: i64,
    /// What the key list shows, never the key.
    pub prefix: String,
    pub source: std::net::IpAddr,
}

/// Resolve an `mk_` bearer key to its owning account + scopes, or
/// `None` if the key is unknown or its record was deleted (revoked).
/// Emits a debug-level reason on every rejection branch so a 401 in
/// prod is diagnosable from logs (`agent_key_miss` /
/// `agent_key_legacy_index` / `agent_key_revoked`).
async fn resolve_agent_key(token: String) -> Option<AgentAuth> {
    // Pooled: this ran on every request carrying an `mk_` key and opened a
    // TCP connection each time. `crate::kevy_pool` keeps eight and hands one
    // out; the URL lives in the pool, so it is no longer threaded in here.
    tokio::task::spawn_blocking(move || -> std::io::Result<Option<AgentAuth>> {
        crate::kevy_pool::with_conn(move |client| {
            let index_key = format!("{AGENT_KEY_INDEX_PREFIX}{token}");
            let Some(raw) = client.get(index_key.as_bytes())? else {
                tracing::debug!(reason = "agent_key_miss", "agent key rejected");
                return Ok(None);
            };
            let index = match serde_json::from_slice::<AgentKeyIndex>(&raw) {
                Ok(i) => i,
                Err(_) => {
                    // legacy index format (bare id, pre user-mapping) —
                    // cannot resolve an owner, treat as invalid;
                    // re-creating the key migrates it
                    tracing::debug!(reason = "agent_key_legacy_index", "agent key rejected");
                    return Ok(None);
                }
            };
            // revocation check: the key is valid only while its record is
            // still present in the owner's hash (delete_agent_key removes it)
            let hash_key = format!("agent:keys:{}", index.user);
            let Some(record) =
                client.hget(hash_key.as_bytes(), index.id.to_string().as_bytes())?
            else {
                tracing::debug!(reason = "agent_key_revoked", user = %index.user, "agent key rejected");
                return Ok(None);
            };
            let scopes = serde_json::from_slice::<AgentKeyRecord>(&record)
                .map(|r| r.scopes)
                .unwrap_or_default();
            // a key must say what it may do; one with no scope (made before
            // scopes were required) authenticates nobody, here or over MCP,
            // where no scope would otherwise read as a browser session
            if scopes.is_empty() {
                tracing::warn!(reason = "agent_key_unscoped", user = %index.user, id = index.id, "agent key rejected");
                return Ok(None);
            }
            Ok(Some(AgentAuth {
                user: index.user,
                scopes,
                key_id: Some(index.id),
            }))
        })
    })
    .await
    .ok()?
    .ok()?
}

/// The signed-in browser session's address, or `None`. An agent key does
/// not count: what asks this (linking a sign-in method) would turn a key
/// into a way to sign in as its owner.
pub async fn resolve_session_user_from_headers(headers: &HeaderMap) -> Option<String> {
    let uri = axum::http::Uri::from_static("/mcp");
    let token = extract_token(headers, &uri)?;
    if token.starts_with(AGENT_KEY_PREFIX) {
        return None;
    }
    resolve_session(token).await.map(|s| s.address)
}

/// The caller behind a session or an agent key, with the key's scopes;
/// the MCP handler uses it to populate its per-session task-local.
pub async fn resolve_auth_from_headers(headers: &HeaderMap) -> Option<AgentAuth> {
    let uri = axum::http::Uri::from_static("/mcp");
    let token = extract_token(headers, &uri)?;
    if token.starts_with(AGENT_KEY_PREFIX) {
        return resolve_agent_key(token).await;
    }
    let session = resolve_session(token).await?;
    Some(AgentAuth {
        user: session.address,
        scopes: Vec::new(),
        key_id: None,
    })
}

/// Extract session token from any of: `mailrs_session` cookie,
/// `Authorization: Bearer <token>` header, or `?token=<hex>` query.
/// The query variant is used by browser <img src> / <a href> which
/// can't set custom headers, e.g. attachment previews.
fn extract_token(headers: &HeaderMap, uri: &axum::http::Uri) -> Option<String> {
    if let Some(cookie_header) = headers.get(axum::http::header::COOKIE)
        && let Ok(raw) = cookie_header.to_str()
    {
        for cookie in raw.split(';') {
            let cookie = cookie.trim();
            if let Some(rest) = cookie.strip_prefix(SESSION_COOKIE)
                && let Some(v) = rest.strip_prefix('=')
            {
                return Some(v.to_string());
            }
        }
    }
    if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION)
        && let Ok(raw) = auth.to_str()
        && let Some(v) = raw.strip_prefix("Bearer ")
    {
        return Some(v.trim().to_string());
    }
    if let Some(q) = uri.query() {
        for pair in q.split('&') {
            if let Some(v) = pair.strip_prefix("token=") {
                // Percent-decode via url crate — session tokens are hex so
                // in practice decode is a noop, but this normalizes anyway.
                return percent_decode(v);
            }
        }
    }
    None
}

fn percent_decode(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let h1 = iter.next()?;
            let h2 = iter.next()?;
            let hex = format!("{}{}", h1 as char, h2 as char);
            let byte = u8::from_str_radix(&hex, 16).ok()?;
            out.push(byte as char);
        } else if b == b'+' {
            out.push(' ');
        } else {
            out.push(b as char);
        }
    }
    Some(out)
}

/// Kevy TTL applied to the session key on every authenticated hit —
/// sliding-window auth so a user actively using the app doesn't get
/// logged out mid-session. Matches the cookie's Max-Age.
const SESSION_TTL_SECS: u64 = 7 * 24 * 3600;

/// Look up the session in kevy. Runs the blocking kevy-client call on a
/// dedicated blocking thread so the axum runtime isn't pinned. On hit,
/// renews the kevy TTL to `SESSION_TTL_SECS`.
async fn resolve_session(token: String) -> Option<SessionBlob> {
    // Pooled, and this is the one that mattered most: it is the per-request
    // cost on **every authenticated call** in the API. Reading one session key
    // used to open a TCP connection to the shared store, and most callers of
    // the old `with_kevy` copies also spawned an OS thread to do it on.
    // `crate::kevy_pool` keeps eight connections and round-robins them.
    let raw = tokio::task::spawn_blocking(move || -> std::io::Result<Option<Vec<u8>>> {
        crate::kevy_pool::with_conn(move |client| {
            let key = format!("{SESSION_KEY_PREFIX}{token}");
            let bytes = client.get(key.as_bytes())?;
            if bytes.is_some() {
                let _ = client.expire(
                    key.as_bytes(),
                    std::time::Duration::from_secs(SESSION_TTL_SECS),
                );
            }
            Ok(bytes)
        })
    })
    .await
    .ok()?
    .ok()??;
    serde_json::from_slice::<SessionBlob>(&raw).ok()
}

/// Axum middleware — extracts the session token from cookie, hits kevy,
/// inserts `AuthedUser` extension. Without a network kevy configured every
/// request is refused; there is no header that names a user.
pub async fn session_auth_middleware(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // no store to resolve a session or key against: nobody is signed in
    if std::env::var(KEVY_URL_ENV).is_err() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let _ = &state; // reserved for future enrichment via core_client

    let token = match extract_token(req.headers(), req.uri()) {
        Some(t) => t,
        None => {
            tracing::debug!(reason = "no_token", path = %req.uri().path(), "request rejected");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // agent API key (mk_…) — machine callers with no browser session
    if token.starts_with(AGENT_KEY_PREFIX) {
        use mailrs_auth_guard::events::{Channel, Credential, LoginEvent, Outcome};
        let crate::login_guard::ClientIp(source) =
            crate::login_guard::ClientIp::of(req.headers(), req.extensions());
        // the prefix the key list shows, never the key
        let key_id: String = token.chars().take(8).collect();
        let guard = &*crate::login_guard::GUARD;
        let auth = match resolve_agent_key(token).await {
            Some(a) => a,
            None => {
                let event = LoginEvent::new(
                    Channel::ApiKey,
                    Credential::ApiKey,
                    source,
                    "",
                    Outcome::BadPassword,
                );
                guard.record(event.with_key(&key_id)).await;
                return Err(StatusCode::UNAUTHORIZED);
            }
        };
        let event = LoginEvent::new(
            Channel::ApiKey,
            Credential::ApiKey,
            source,
            &auth.user,
            Outcome::Ok,
        );
        guard.record(event.with_key(&key_id)).await;
        if !agent_scopes_allow(&auth.scopes, req.method(), req.uri().path()) {
            tracing::debug!(
                reason = "agent_key_scope",
                user = %auth.user,
                scopes = ?auth.scopes,
                method = %req.method(),
                path = %req.uri().path(),
                "agent key denied by scopes"
            );
            return Err(StatusCode::FORBIDDEN);
        }
        req.extensions_mut()
            .insert(crate::handlers::conversations::AuthedDisplayName::default());
        if let Some(id) = auth.key_id {
            req.extensions_mut().insert(AgentKey {
                id,
                prefix: key_id,
                source,
            });
        }
        req.extensions_mut().insert(AuthedUser(auth.user));
        req.extensions_mut().insert(AgentScopes(auth.scopes));
        return Ok(next.run(req).await);
    }

    let session = match resolve_session(token).await {
        Some(s) => s,
        None => {
            tracing::debug!(reason = "session_miss", path = %req.uri().path(), "request rejected");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    req.extensions_mut()
        .insert(crate::handlers::conversations::AuthedDisplayName(
            session.display_name.clone(),
        ));
    req.extensions_mut().insert(AuthedUser(session.address));
    req.extensions_mut().insert(AgentScopes::default());
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn h(cookie: &str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_str(cookie).unwrap(),
        );
        m
    }

    #[test]
    fn parses_lone_session_cookie() {
        let m = h("mailrs_session=abc123");
        assert_eq!(
            extract_token(&m, &axum::http::Uri::from_static("/")).as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn parses_session_among_others() {
        let m = h("foo=bar; mailrs_session=xyz; baz=qux");
        assert_eq!(
            extract_token(&m, &axum::http::Uri::from_static("/")).as_deref(),
            Some("xyz")
        );
    }

    #[test]
    fn missing_cookie_returns_none() {
        let m = h("foo=bar");
        assert!(extract_token(&m, &axum::http::Uri::from_static("/")).is_none());
    }

    #[test]
    fn empty_session_value_yields_empty_string() {
        let m = h("mailrs_session=");
        assert_eq!(
            extract_token(&m, &axum::http::Uri::from_static("/")).as_deref(),
            Some("")
        );
    }

    #[test]
    fn agent_key_index_parses_user_and_id() {
        let raw = br#"{"user":"lihao@golia.jp","id":3}"#;
        let idx: AgentKeyIndex = serde_json::from_slice(raw).unwrap();
        assert_eq!(idx.user, "lihao@golia.jp");
        assert_eq!(idx.id, 3);
    }

    #[test]
    fn agent_key_index_rejects_legacy_bare_id() {
        // pre user-mapping index stored the bare id — must fail to parse
        // so the middleware treats the key as invalid instead of panicking
        assert!(serde_json::from_slice::<AgentKeyIndex>(b"7").is_err());
    }

    #[test]
    fn bearer_agent_key_extracted_and_recognized() {
        let mut m = HeaderMap::new();
        m.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer mk_abcdef0123456789"),
        );
        let token = extract_token(&m, &axum::http::Uri::from_static("/")).unwrap();
        assert!(token.starts_with(AGENT_KEY_PREFIX));
    }
}
