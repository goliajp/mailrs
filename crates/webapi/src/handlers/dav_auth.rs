//! Login for CalDAV and CardDAV clients.
//!
//! Calendar and address-book clients send `Authorization: Basic` on every
//! request and only send it after a 401 carrying a `WWW-Authenticate`
//! challenge; a session cookie or bearer token is nothing they can obtain.
//! A session or bearer still works here, so the web client reaches `/dav`
//! the way it reaches everything else.
//!
//! Like IMAP, a Basic login is the account password alone: an account with
//! a second factor enrolled is not asked for it here.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use mailrs_auth_guard::login::Verdict;

use crate::WebState;
use crate::handlers::conversations::{AuthedDisplayName, AuthedUser};
use crate::login_guard::ClientIp;
use crate::session::AgentScopes;

const CHALLENGE: &str = "Basic realm=\"mailrs\", charset=\"UTF-8\"";

pub async fn dav_auth_middleware(
    State(state): State<Arc<WebState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(credentials) = basic_credentials(req.headers()) else {
        return match crate::session::session_auth_middleware(State(state), req, next).await {
            Ok(resp) => resp,
            Err(StatusCode::UNAUTHORIZED) => challenge(),
            Err(status) => status.into_response(),
        };
    };
    let Some((address, password)) = credentials else {
        return challenge();
    };
    let ClientIp(source) = ClientIp::of(req.headers(), req.extensions());
    let guard = &*crate::login_guard::GUARD;
    let verdict = guard.check(source, &address).await;
    if let Verdict::Locked { .. } = verdict {
        tracing::warn!(event = "auth", channel = "dav", %source, account = %mailrs_auth_guard::login::loggable_account(&address), result = "locked");
        return crate::login_guard::locked_response(verdict);
    }
    match crate::handlers::auth::password::verify(&state, &address, &password).await {
        Ok(Some(acct)) => {
            guard.succeeded(source, &address).await;
            req.extensions_mut().insert(AuthedDisplayName::default());
            req.extensions_mut().insert(AuthedUser(acct.public.address));
            req.extensions_mut().insert(AgentScopes::default());
            next.run(req).await
        }
        Ok(None) => {
            tracing::warn!(event = "auth", channel = "dav", %source, account = %mailrs_auth_guard::login::loggable_account(&address), result = "bad-password");
            guard.failed(source, &address).await;
            challenge()
        }
        Err(status) => status.into_response(),
    }
}

fn challenge() -> Response {
    let mut resp = StatusCode::UNAUTHORIZED.into_response();
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static(CHALLENGE),
    );
    resp
}

/// `None` when the request carries no Basic credentials, `Some(None)` when
/// it carries a Basic header that does not decode to `user:password`.
fn basic_credentials(headers: &HeaderMap) -> Option<Option<(String, String)>> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = raw.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok());
    Some(decoded.and_then(|s| {
        let (user, pass) = s.split_once(':')?;
        Some((user.trim().to_string(), pass.to_string()))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_auth(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn basic_credentials_decode() {
        // "alice@x.test:p:ss" — the password may contain a colon
        let h = with_auth("Basic YWxpY2VAeC50ZXN0OnA6c3M=");
        assert_eq!(
            basic_credentials(&h),
            Some(Some(("alice@x.test".into(), "p:ss".into())))
        );
        assert_eq!(basic_credentials(&with_auth("Basic !!!")), Some(None));
        assert_eq!(basic_credentials(&with_auth("Bearer abc")), None);
        assert_eq!(basic_credentials(&HeaderMap::new()), None);
    }
}
