//! Login for JMAP: a session or agent key as on the rest of the API, or an
//! app password over HTTP Basic the way IMAP takes one, under the same
//! per-account rule (an account set to app passwords only refuses its
//! account password here too) and the same lockout.
//!
//! Basic is refused unless the client reached us over HTTPS: the
//! password would otherwise have crossed the network in the clear.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use mailrs_auth_guard::events::Channel;

use crate::WebState;
use crate::handlers::dav_auth;

pub async fn jmap_auth_middleware(
    State(state): State<Arc<WebState>>,
    req: Request,
    next: Next,
) -> Response {
    let basic = dav_auth::basic_credentials(req.headers()).is_some();
    if basic && !crate::login_guard::request_is_https(req.headers(), req.extensions()) {
        return (
            StatusCode::FORBIDDEN,
            "a password is accepted over HTTPS only",
        )
            .into_response();
    }
    dav_auth::basic_or_session(state, Channel::Jmap, req, next).await
}
