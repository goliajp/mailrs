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
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

pub mod confirm;
pub mod csrf;
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
use mailrs_core_sidestate::keys::session::session_key;

/// Agent API keys (`mk_…`, created via POST /api/agent/keys) are an
/// alternative bearer credential for machine callers. The secret index
/// (`agent_key_index`, by the key's hash) stores `{"user":…,"id":…}`; the key is live
/// only while its record still exists in the `agent:keys:<user>` hash,
/// so deletion revokes immediately even though the index lingers.
pub(crate) const AGENT_KEY_PREFIX: &str = "mk_";

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
    /// The session's hash; `None` for an agent key.
    pub session: Option<confirm::SessionRef>,
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
            let index_key = crate::agent_key_index::index_key(&token);
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
            let hash_key = mailrs_core_sidestate::keys::credentials::agent_keys_key(&index.user);
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
            let now = chrono::Utc::now().timestamp();
            if let Err(e) = crate::agent_key_usage::note_use(client, &index.user, index.id, now) {
                tracing::warn!(error = %e, user = %index.user, "agent key use not recorded");
            }
            Ok(Some(AgentAuth {
                user: index.user,
                scopes,
                key_id: Some(index.id),
                session: None,
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
pub async fn resolve_session_user_from_headers(
    headers: &HeaderMap,
) -> Option<(String, confirm::SessionRef)> {
    let token = extract_token(headers)?;
    if token.starts_with(AGENT_KEY_PREFIX) {
        return None;
    }
    let session = confirm::SessionRef(mailrs_core_sidestate::keys::session::session_ref(&token));
    resolve_session(token).await.map(|s| (s.address, session))
}

/// The signed-in browser session behind a request that the auth middleware
/// does not cover, such as a WebSocket upgrade. Same token sources as the
/// middleware; a browser sends the session cookie on a same-origin upgrade,
/// so nothing has to be put in the URL. Agent keys do not count.
pub async fn resolve_session_user(headers: &HeaderMap) -> Option<(String, TokenSource)> {
    let (token, source) = extract_token_with_source(headers)?;
    if token.starts_with(AGENT_KEY_PREFIX) {
        return None;
    }
    resolve_session(token).await.map(|s| (s.address, source))
}

/// The check every WebSocket upgrade makes. An upgrade is a GET, so the
/// CSRF token does not apply; one the cookie authenticates must instead
/// name one of our web origins. A browser always sends `Origin` on an
/// upgrade, so one that sends none is not a browser and should use a
/// bearer token.
pub fn websocket_origin_ok(headers: &HeaderMap, source: TokenSource) -> bool {
    if source != TokenSource::Cookie {
        return true;
    }
    csrf::origin_ok(headers, &csrf::config().origins) == Some(true)
}

/// The caller behind a session or an agent key, with the key's scopes;
/// the MCP handler uses it to populate its per-session task-local. Only an
/// `Authorization` header counts: no browser page uses MCP, and a cookie
/// would let any page the user visits call it.
pub async fn resolve_auth_from_headers(headers: &HeaderMap) -> Option<AgentAuth> {
    let (token, source) = extract_token_with_source(headers)?;
    if source != TokenSource::Header {
        return None;
    }
    if token.starts_with(AGENT_KEY_PREFIX) {
        return resolve_agent_key(token).await;
    }
    let session = resolve_session(token.clone()).await?;
    Some(AgentAuth {
        user: session.address,
        scopes: Vec::new(),
        key_id: None,
        session: Some(confirm::SessionRef(
            mailrs_core_sidestate::keys::session::session_ref(&token),
        )),
    })
}

/// Where a request's credential came from. Only the cookie needs CSRF
/// checks: a page on another site can make a browser send it, but cannot
/// set a header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenSource {
    Cookie,
    Header,
}

fn extract_token(headers: &HeaderMap) -> Option<String> {
    extract_token_with_source(headers).map(|(t, _)| t)
}

/// The token from an `Authorization: Bearer` header or the `mailrs_session`
/// cookie, in that order. Never from the URL: a token there ends up in
/// proxy logs, browser history and `Referer` headers. A request with
/// an `Authorization` header is judged by it alone: a bad one is a 401 and
/// never falls back to the cookie, or junk in the header would buy a
/// cross-site request the cookie's authority without its CSRF check.
fn extract_token_with_source(headers: &HeaderMap) -> Option<(String, TokenSource)> {
    if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION) {
        let v = auth.to_str().ok()?.strip_prefix("Bearer ")?.trim();
        return Some((v.to_string(), TokenSource::Header));
    }
    if let Some(cookie_header) = headers.get(axum::http::header::COOKIE)
        && let Ok(raw) = cookie_header.to_str()
    {
        for cookie in raw.split(';') {
            let cookie = cookie.trim();
            if let Some(rest) = cookie.strip_prefix(SESSION_COOKIE)
                && let Some(v) = rest.strip_prefix('=')
            {
                return Some((v.to_string(), TokenSource::Cookie));
            }
        }
    }
    None
}

/// Kevy TTL applied to the session key on every authenticated hit —
/// sliding-window auth so a user actively using the app doesn't get
/// logged out mid-session. Matches the cookie's Max-Age.
pub(crate) const SESSION_TTL_SECS: u64 = 7 * 24 * 3600;

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
            let key = session_key(&token);
            let bytes = client.get(key.as_bytes())?;
            if bytes.is_some() {
                let _ = client.expire(
                    key.as_bytes(),
                    std::time::Duration::from_secs(SESSION_TTL_SECS),
                );
                return Ok(bytes);
            }
            move_legacy_session(client, &token)
        })
    })
    .await
    .ok()?
    .ok()??;
    serde_json::from_slice::<SessionBlob>(&raw).ok()
}

/// A session issued before sessions were stored by hash, moved to its
/// hashed key on first use.
///
/// The network client has no transaction on every backend, so the steps
/// are ordered so a logout or a revocation running alongside cannot bring
/// the session back: the hashed copy is written first and dropped again if
/// the original has meanwhile been deleted; the index gains the hash before
/// it loses the token, so a revocation always finds one of the two, and
/// revocation deletes the hashed key for either.
fn move_legacy_session(
    client: &mut kevy_client::Connection,
    token: &str,
) -> std::io::Result<Option<Vec<u8>>> {
    let legacy = mailrs_core_sidestate::keys::session::legacy_session_key(token);
    let Some(bytes) = client.get(legacy.as_bytes())? else {
        return Ok(None);
    };
    write_moved(client, token, &bytes)?;
    settle_move(client, token, bytes)
}

// the steps of `move_legacy_session`, apart so the tests can end the
// session between them
fn write_moved(
    client: &mut kevy_client::Connection,
    token: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    client.set_with_ttl(
        session_key(token).as_bytes(),
        bytes,
        std::time::Duration::from_secs(SESSION_TTL_SECS),
    )?;
    Ok(())
}

fn settle_move(
    client: &mut kevy_client::Connection,
    token: &str,
    bytes: Vec<u8>,
) -> std::io::Result<Option<Vec<u8>>> {
    use mailrs_core_sidestate::keys::session::{by_addr_key, legacy_session_key, session_ref};
    let legacy = legacy_session_key(token);
    let stored = session_key(token);
    if client.exists(&[legacy.as_bytes()])? == 0 {
        client.del(&[stored.as_bytes()])?;
        return Ok(None);
    }
    // the token leaves the index last: while it is there a revocation
    // deletes the old key, which a later request would otherwise move back
    let idx = serde_json::from_slice::<SessionBlob>(&bytes)
        .ok()
        .map(|blob| by_addr_key(&blob.address));
    if let Some(idx) = &idx {
        client.sadd(idx.as_bytes(), &[session_ref(token).as_bytes()])?;
    }
    client.del(&[legacy.as_bytes()])?;
    if let Some(idx) = &idx {
        client.srem(idx.as_bytes(), &[token.as_bytes()])?;
    }
    Ok(Some(bytes))
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

    let (token, token_source) = match extract_token_with_source(req.headers()) {
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

    let session = match resolve_session(token.clone()).await {
        Some(s) => s,
        None => {
            tracing::debug!(reason = "session_miss", path = %req.uri().path(), "request rejected");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    let by_cookie = token_source == TokenSource::Cookie;
    // the token cookie is (re)issued whenever it is missing, refusals
    // included, so a session from before CSRF checks gets one on first use
    let issue_cookie = by_cookie && !csrf::has_cookie(req.headers(), &token);
    if by_cookie {
        let cfg = csrf::config();
        if let Err(v) = csrf::check(req.method(), req.headers(), &token, &cfg.origins) {
            // what was missing or wrong, never a value
            tracing::warn!(
                violation = v.code(),
                method = %req.method(),
                path = %req.uri().path(),
                enforced = cfg.mode == csrf::Mode::Enforce,
                "csrf"
            );
            if cfg.mode == csrf::Mode::Enforce {
                let mut resp = (
                    StatusCode::FORBIDDEN,
                    axum::Json(serde_json::json!({ "error": v.code() })),
                )
                    .into_response();
                if issue_cookie {
                    resp.headers_mut()
                        .append(axum::http::header::SET_COOKIE, csrf::set_cookie(&token));
                }
                return Ok(resp);
            }
        }
    }

    req.extensions_mut()
        .insert(crate::handlers::conversations::AuthedDisplayName(
            session.display_name.clone(),
        ));
    req.extensions_mut().insert(AuthedUser(session.address));
    req.extensions_mut().insert(AgentScopes::default());
    req.extensions_mut().insert(confirm::SessionRef(
        mailrs_core_sidestate::keys::session::session_ref(&token),
    ));
    let mut resp = next.run(req).await;
    if issue_cookie {
        resp.headers_mut()
            .append(axum::http::header::SET_COOKIE, csrf::set_cookie(&token));
    }
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use mailrs_core_sidestate::keys::session::{
        by_addr_key, legacy_session_key, session_key_by_ref, session_ref,
    };

    const OWNER: &str = "a@x.test";

    /// A session as a release before hashing left it, on its own store.
    fn legacy(name: &str, token: &str) -> kevy_client::Connection {
        let mut c =
            kevy_client::Connection::connect(&format!("mem://session-move-{name}")).unwrap();
        c.set(
            legacy_session_key(token).as_bytes(),
            br#"{"address":"a@x.test","display_name":""}"#,
        )
        .unwrap();
        c.sadd(by_addr_key(OWNER).as_bytes(), &[token.as_bytes()])
            .unwrap();
        c
    }

    /// Nothing under any name the session could have, and a later request
    /// does not find it either.
    fn gone(c: &mut kevy_client::Connection, token: &str) {
        for key in [session_key(token), legacy_session_key(token)] {
            assert_eq!(c.exists(&[key.as_bytes()]).unwrap(), 0, "{key} is back");
        }
        assert!(super::move_legacy_session(c, token).unwrap().is_none());
    }

    #[test]
    fn a_logout_before_the_copy_is_written_stays_a_logout() {
        let t = "1".repeat(64);
        let mut c = legacy("logout-before-write", &t);
        let bytes = c.get(legacy_session_key(&t).as_bytes()).unwrap().unwrap();
        crate::handlers::auth::end_session(&mut c, &t).unwrap();
        super::write_moved(&mut c, &t, &bytes).unwrap();
        assert!(super::settle_move(&mut c, &t, bytes).unwrap().is_none());
        gone(&mut c, &t);
    }

    #[test]
    fn a_logout_after_the_copy_is_written_stays_a_logout() {
        let t = "2".repeat(64);
        let mut c = legacy("logout-after-write", &t);
        let bytes = c.get(legacy_session_key(&t).as_bytes()).unwrap().unwrap();
        super::write_moved(&mut c, &t, &bytes).unwrap();
        crate::handlers::auth::end_session(&mut c, &t).unwrap();
        assert!(super::settle_move(&mut c, &t, bytes).unwrap().is_none());
        gone(&mut c, &t);
    }

    #[test]
    fn a_revocation_after_the_copy_is_written_stays_a_revocation() {
        let t = "3".repeat(64);
        let mut c = legacy("revoke-after-write", &t);
        let bytes = c.get(legacy_session_key(&t).as_bytes()).unwrap().unwrap();
        super::write_moved(&mut c, &t, &bytes).unwrap();
        crate::handlers::auth::revoke_sessions(&mut c, OWNER).unwrap();
        assert!(super::settle_move(&mut c, &t, bytes).unwrap().is_none());
        gone(&mut c, &t);
    }

    /// The rest of the move is three writes: the hash joins the index, the
    /// old key goes, the token leaves the index. A revocation after either
    /// of the first two still finds the session.
    #[test]
    fn a_revocation_during_the_index_swap_finds_the_session() {
        for (n, old_key_dropped) in [(4, false), (5, true)] {
            let t = n.to_string().repeat(64);
            let mut c = legacy(&format!("revoke-during-swap-{n}"), &t);
            let bytes = c.get(legacy_session_key(&t).as_bytes()).unwrap().unwrap();
            super::write_moved(&mut c, &t, &bytes).unwrap();
            c.sadd(by_addr_key(OWNER).as_bytes(), &[session_ref(&t).as_bytes()])
                .unwrap();
            if old_key_dropped {
                c.del(&[legacy_session_key(&t).as_bytes()]).unwrap();
            }
            crate::handlers::auth::revoke_sessions(&mut c, OWNER).unwrap();
            assert_eq!(
                c.exists(&[session_key_by_ref(&session_ref(&t)).as_bytes()])
                    .unwrap(),
                0
            );
            gone(&mut c, &t);
        }
    }

    #[test]
    fn an_undisturbed_move_keeps_the_session() {
        let t = "6".repeat(64);
        let mut c = legacy("undisturbed", &t);
        assert!(super::move_legacy_session(&mut c, &t).unwrap().is_some());
        assert_eq!(c.exists(&[session_key(&t).as_bytes()]).unwrap(), 1);
        assert_eq!(c.exists(&[legacy_session_key(&t).as_bytes()]).unwrap(), 0);
    }

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
        assert_eq!(extract_token(&m).as_deref(), Some("abc123"));
    }

    #[test]
    fn parses_session_among_others() {
        let m = h("foo=bar; mailrs_session=xyz; baz=qux");
        assert_eq!(extract_token(&m).as_deref(), Some("xyz"));
    }

    #[test]
    fn missing_cookie_returns_none() {
        let m = h("foo=bar");
        assert!(extract_token(&m).is_none());
    }

    #[test]
    fn empty_session_value_yields_empty_string() {
        let m = h("mailrs_session=");
        assert_eq!(extract_token(&m).as_deref(), Some(""));
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
        let token = extract_token(&m).unwrap();
        assert!(token.starts_with(AGENT_KEY_PREFIX));
    }
}
