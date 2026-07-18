//! Session resolution against the shared kevy-server.
//!
//! Phase 3.9 — replaces the temporary `X-Mailrs-User` header stub with
//! the real `mailrs_session` cookie lookup. The session blob stored by
//! the monolith webapi (see `crates/server/src/web/mod.rs:124`) is JSON:
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
const SESSION_KEY_PREFIX: &str = "session:";

/// Agent API keys (`mk_…`, created via POST /api/agent/keys) are an
/// alternative bearer credential for machine callers. The secret index
/// `agent:key:secret:<key>` stores `{"user":…,"id":…}`; the key is live
/// only while its record still exists in the `agent:keys:<user>` hash,
/// so deletion revokes immediately even though the index lingers.
const AGENT_KEY_PREFIX: &str = "mk_";
const AGENT_KEY_INDEX_PREFIX: &str = "agent:key:secret:";

#[derive(Debug, Deserialize)]
struct AgentKeyIndex {
    user: String,
    id: i64,
}

/// The scopes stored on the key record. Empty = full owner access
/// (user decision 2026-07-18: existing keys keep working); a non-empty
/// list is enforced — see [`agent_scopes_allow`].
#[derive(Debug, Deserialize)]
struct AgentKeyRecord {
    #[serde(default)]
    scopes: Vec<String>,
}

/// An authenticated agent key: owner + its (possibly empty) scope set.
pub struct AgentAuth {
    pub user: String,
    pub scopes: Vec<String>,
}

/// Scope vocabulary: `admin` (everything), `mail:write` (read + write,
/// no /api/admin), `mail:read` (GET/HEAD only, no /api/admin). An empty
/// scope list grants full owner access.
fn agent_scopes_allow(scopes: &[String], method: &axum::http::Method, path: &str) -> bool {
    if scopes.is_empty() || scopes.iter().any(|s| s == "admin") {
        return true;
    }
    if path.starts_with("/api/admin") {
        return false;
    }
    let read_only = matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD);
    if scopes.iter().any(|s| s == "mail:write") {
        return true;
    }
    scopes.iter().any(|s| s == "mail:read") && read_only
}

/// Resolve an `mk_` bearer key to its owning account + scopes, or
/// `None` if the key is unknown or its record was deleted (revoked).
/// Emits a debug-level reason on every rejection branch so a 401 in
/// prod is diagnosable from logs (`agent_key_miss` /
/// `agent_key_legacy_index` / `agent_key_revoked`).
async fn resolve_agent_key(kevy_url: String, token: String) -> Option<AgentAuth> {
    tokio::task::spawn_blocking(move || -> std::io::Result<Option<AgentAuth>> {
        let mut client = kevy_client::Connection::open(&kevy_url)?;
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
        let Some(record) = client.hget(hash_key.as_bytes(), index.id.to_string().as_bytes())?
        else {
            tracing::debug!(reason = "agent_key_revoked", user = %index.user, "agent key rejected");
            return Ok(None);
        };
        let scopes = serde_json::from_slice::<AgentKeyRecord>(&record)
            .map(|r| r.scopes)
            .unwrap_or_default();
        Ok(Some(AgentAuth {
            user: index.user,
            scopes,
        }))
    })
    .await
    .ok()?
    .ok()?
}

/// Extract session token from any of: `mailrs_session` cookie,
/// `Authorization: Bearer <token>` header, or `?token=<hex>` query.
/// The query variant is used by browser <img src> / <a href> which
/// can't set custom headers, e.g. attachment previews.
/// Resolve the authenticated user address from the incoming headers,
/// mirroring [`session_auth_middleware`] but returning the result
/// instead of mutating request extensions. Used by the MCP handler
/// to populate its per-session task-local. Returns `None` on any
/// auth failure — the caller decides how to react.
pub async fn resolve_user_from_headers(headers: &HeaderMap) -> Option<String> {
    let kevy_url = std::env::var(KEVY_URL_ENV).ok();
    let Some(kevy_url) = kevy_url else {
        // Dev fallback matches session_auth_middleware.
        return headers
            .get("X-Mailrs-User")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
    };
    let uri = axum::http::Uri::from_static("/mcp");
    let token = extract_token(headers, &uri)?;
    if token.starts_with(AGENT_KEY_PREFIX) {
        return resolve_agent_key(kevy_url, token).await.map(|a| a.user);
    }
    let session = resolve_session(kevy_url, token).await?;
    Some(session.address)
}

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
async fn resolve_session(kevy_url: String, token: String) -> Option<SessionBlob> {
    let token_clone = token.clone();
    let raw = tokio::task::spawn_blocking(move || -> std::io::Result<Option<Vec<u8>>> {
        let mut client = kevy_client::Connection::open(&kevy_url)?;
        let key = format!("{SESSION_KEY_PREFIX}{token_clone}");
        let bytes = client.get(key.as_bytes())?;
        if bytes.is_some() {
            let _ = client.expire(
                key.as_bytes(),
                std::time::Duration::from_secs(SESSION_TTL_SECS),
            );
        }
        Ok(bytes)
    })
    .await
    .ok()?
    .ok()??;
    serde_json::from_slice::<SessionBlob>(&raw).ok()
}

/// Axum middleware — extracts the session token from cookie, hits kevy,
/// inserts `AuthedUser` extension. Falls back to the legacy
/// `X-Mailrs-User` header in dev mode (when `MAILRS_KEVY_URL` is unset).
pub async fn session_auth_middleware(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Dev fallback — header-based auth when no kevy is wired up.
    let kevy_url = std::env::var(KEVY_URL_ENV).ok();
    if kevy_url.is_none() {
        let user_opt = req
            .headers()
            .get("X-Mailrs-User")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        match user_opt {
            Some(user) => {
                req.extensions_mut()
                    .insert(crate::handlers::conversations::AuthedDisplayName::default());
                req.extensions_mut().insert(AuthedUser(user));
                return Ok(next.run(req).await);
            }
            None => return Err(StatusCode::UNAUTHORIZED),
        }
    }
    let kevy_url = kevy_url.expect("checked above");
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
        let auth = match resolve_agent_key(kevy_url, token).await {
            Some(a) => a,
            None => return Err(StatusCode::UNAUTHORIZED),
        };
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
        req.extensions_mut().insert(AuthedUser(auth.user));
        return Ok(next.run(req).await);
    }

    let session = match resolve_session(kevy_url, token).await {
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
    fn empty_scopes_grant_full_access() {
        let m = axum::http::Method::POST;
        assert!(agent_scopes_allow(&[], &m, "/api/admin/domains"));
        assert!(agent_scopes_allow(
            &[],
            &axum::http::Method::GET,
            "/api/conversations"
        ));
    }

    #[test]
    fn admin_scope_grants_everything() {
        let scopes = vec!["admin".to_string()];
        assert!(agent_scopes_allow(
            &scopes,
            &axum::http::Method::DELETE,
            "/api/admin/aliases/x"
        ));
    }

    #[test]
    fn mail_read_is_get_only_and_never_admin() {
        let scopes = vec!["mail:read".to_string()];
        assert!(agent_scopes_allow(
            &scopes,
            &axum::http::Method::GET,
            "/api/conversations"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &axum::http::Method::POST,
            "/api/mail/send"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &axum::http::Method::GET,
            "/api/admin/accounts"
        ));
    }

    #[test]
    fn mail_write_allows_writes_but_not_admin() {
        let scopes = vec!["mail:write".to_string()];
        assert!(agent_scopes_allow(
            &scopes,
            &axum::http::Method::POST,
            "/api/mail/send"
        ));
        assert!(agent_scopes_allow(
            &scopes,
            &axum::http::Method::GET,
            "/api/conversations"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &axum::http::Method::GET,
            "/api/admin/accounts"
        ));
    }

    #[test]
    fn unknown_scope_denies_writes() {
        // a key created with scopes: ["read"] (not in the vocabulary)
        // must not silently gain write access
        let scopes = vec!["read".to_string()];
        assert!(!agent_scopes_allow(
            &scopes,
            &axum::http::Method::POST,
            "/api/mail/send"
        ));
        assert!(!agent_scopes_allow(
            &scopes,
            &axum::http::Method::GET,
            "/api/conversations"
        ));
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
