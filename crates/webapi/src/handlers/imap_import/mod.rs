//! Importing mail from another provider over IMAP.
//!
//! `POST /api/mail/import/imap` starts it, `GET` says where it stands,
//! `DELETE` stops it. Every message goes through the same filing as an
//! mbox upload (`crate::handlers::mbox::import::Filer`): judged and
//! screened on the way in, Junk and Trash left behind, duplicates skipped.

pub mod client;
pub mod connect;
pub mod job;
pub mod plan;

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// Request body for `POST /api/mail/import/imap`.
#[derive(serde::Deserialize)]
pub struct StartRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

fn refused(status: StatusCode, why: &str) -> Response {
    (status, Json(serde_json::json!({ "error": why }))).into_response()
}

/// A host name as a user may type one: letters, digits, dots and hyphens,
/// or an address. Anything else is not a server to connect to.
fn host_of(raw: &str) -> Option<String> {
    let host = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    let fits = !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    fits.then_some(host)
}

/// POST /api/mail/import/imap
pub async fn start(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<StartRequest>,
) -> Response {
    let Some(host) = host_of(&req.host) else {
        return refused(StatusCode::BAD_REQUEST, "not a host name");
    };
    if req.username.is_empty()
        || req.password.is_empty()
        || req.username.len() > 1024
        || req.password.len() > 1024
    {
        return refused(
            StatusCode::BAD_REQUEST,
            "a user name and a password are needed",
        );
    }
    let source = job::Source {
        host,
        port: req.port,
        username: req.username,
        password: req.password,
    };
    match job::start(state, &user, source, Arc::new(connect::Checked)) {
        Ok(status) => (StatusCode::ACCEPTED, Json(status)).into_response(),
        Err(job::Refused::AlreadyRunning) => {
            refused(StatusCode::CONFLICT, "an import is already running")
        }
        Err(job::Refused::TooManyToday) => refused(
            StatusCode::TOO_MANY_REQUESTS,
            "five imports have been started today; try again tomorrow",
        ),
        Err(job::Refused::Invalid(why)) => refused(StatusCode::BAD_REQUEST, &why),
    }
}

/// GET /api/mail/import/imap
pub async fn status(Extension(AuthedUser(user)): Extension<AuthedUser>) -> Json<job::Status> {
    Json(job::status(&user))
}

/// DELETE /api/mail/import/imap
pub async fn cancel(Extension(AuthedUser(user)): Extension<AuthedUser>) -> StatusCode {
    match job::cancel(&user) {
        true => StatusCode::NO_CONTENT,
        false => StatusCode::NOT_FOUND,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_a_name_or_an_address() {
        assert_eq!(
            host_of(" IMAP.Gmail.com. ").as_deref(),
            Some("imap.gmail.com")
        );
        assert_eq!(host_of("2001:db8::1").as_deref(), Some("2001:db8::1"));
        for bad in [
            "",
            "imap.example.com/x",
            "a b",
            "imap@example.com",
            "x\r\ny",
        ] {
            assert_eq!(host_of(bad), None, "{bad:?}");
        }
    }
}
