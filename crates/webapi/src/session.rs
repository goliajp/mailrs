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

/// `session:<token>` blob — only the fields webapi needs.
#[derive(Debug, Deserialize)]
struct SessionBlob {
    address: String,
}

/// kevy connection URL env var (matches the monolith's
/// `MAILRS_KEVY_URL`).
const KEVY_URL_ENV: &str = "MAILRS_KEVY_URL";

const SESSION_COOKIE: &str = "mailrs_session";
const SESSION_KEY_PREFIX: &str = "session:";

/// Pull `mailrs_session=<token>` out of the `Cookie:` header.
fn extract_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for cookie in raw.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix(SESSION_COOKIE) {
            // strip the `=` plus the value
            if let Some(stripped) = token.strip_prefix('=') {
                return Some(stripped.to_string());
            }
        }
    }
    None
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
                req.extensions_mut().insert(AuthedUser(user));
                return Ok(next.run(req).await);
            }
            None => return Err(StatusCode::UNAUTHORIZED),
        }
    }
    let kevy_url = kevy_url.expect("checked above");
    let _ = &state; // reserved for future enrichment via core_client

    let token = {
        let h = req.headers();
        match extract_token(h) {
            Some(t) => t,
            None => return Err(StatusCode::UNAUTHORIZED),
        }
    };

    let session = match resolve_session(kevy_url, token).await {
        Some(s) => s,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

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
}
