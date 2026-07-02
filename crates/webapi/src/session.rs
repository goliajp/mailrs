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

/// Look up the session in kevy. Runs the blocking kevy-client call on a
/// dedicated blocking thread so the axum runtime isn't pinned.
async fn resolve_session(kevy_url: String, token: String) -> Option<SessionBlob> {
    let token_clone = token.clone();
    let raw = tokio::task::spawn_blocking(move || -> std::io::Result<Option<Vec<u8>>> {
        let mut client = kevy_client::Connection::open(&kevy_url)?;
        let key = format!("{SESSION_KEY_PREFIX}{token_clone}");
        client.get(key.as_bytes())
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
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let session = match resolve_session(kevy_url, token).await {
        Some(s) => s,
        None => return Err(StatusCode::UNAUTHORIZED),
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
}
