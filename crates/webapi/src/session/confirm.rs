//! Recent confirmation: typing the password again before an operation a
//! stolen session could use to stay in or to carry mail away.
//!
//! A session gains a mark by passing `POST /api/auth/confirm`. The mark
//! lasts ten minutes from then and is not extended by use; ending the
//! session ends it too. A handler that takes [`Confirmed`] answers 403
//! `confirm_required` without one, and 403 `agent_key_not_allowed` to an
//! agent key, which has no password to confirm.
//!
//! Confirmation goes through the login guard: a locked account or source
//! cannot confirm, and every failure counts toward the same lockout as a
//! failed sign-in. Five failures on one session end that session.

use std::sync::Arc;

use axum::extract::{Extension, FromRequestParts, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use mailrs_auth_guard::events::{Channel, Credential, LoginEvent, Outcome};
use mailrs_auth_guard::login::Verdict;
use mailrs_core_sidestate::keys::session::{
    by_addr_key, confirm_failures_key, confirmed_key, session_key_by_ref,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::totp_util::{SecondFactor, second_factor};
use crate::login_guard::ClientIp;

/// How long a confirmation lasts.
pub const CONFIRM_TTL: std::time::Duration = std::time::Duration::from_secs(600);
/// Failed confirmations that end the session they were made on.
pub const FAILURES_TO_END: i64 = 5;

/// The hash of the session a request was authenticated by. Present on
/// every request a session authenticates, absent on one an agent key does.
#[derive(Clone, Debug)]
pub struct SessionRef(pub String);

/// Proof that the request's session confirmed its password recently.
pub struct Confirmed(());

/// Why an operation that needs a confirmation was refused.
#[derive(Debug)]
pub enum Refusal {
    /// The request came with an agent key, which has no password.
    AgentKeyNotAllowed,
    /// The session has not confirmed recently.
    ConfirmRequired,
    /// The store that holds the marks could not be read.
    Unavailable(StatusCode),
}

impl Refusal {
    /// The error code the response carries.
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::AgentKeyNotAllowed => "agent_key_not_allowed",
            Refusal::ConfirmRequired => "confirm_required",
            Refusal::Unavailable(_) => "unavailable",
        }
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Refusal::Unavailable(status) => status.into_response(),
            other => refusal(other.code()),
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Confirmed {
    type Rejection = Refusal;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let session = parts.extensions.get::<SessionRef>().cloned();
        require(session.as_ref())
    }
}

/// The check behind [`Confirmed`], for callers that are not extractors
/// (the MCP tools, the external-login callback).
pub fn require(session: Option<&SessionRef>) -> Result<Confirmed, Refusal> {
    let Some(SessionRef(session)) = session else {
        return Err(Refusal::AgentKeyNotAllowed);
    };
    let key = confirmed_key(session);
    let marked =
        with_kevy(move |c| Ok(c.exists(&[key.as_bytes()])? == 1)).map_err(Refusal::Unavailable)?;
    if !marked {
        return Err(Refusal::ConfirmRequired);
    }
    Ok(Confirmed(()))
}

fn refusal(code: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({ "error": code })),
    )
        .into_response()
}

fn unauthorized(body: serde_json::Value) -> Response {
    (StatusCode::UNAUTHORIZED, axum::Json(body)).into_response()
}

#[derive(serde::Deserialize)]
pub struct ConfirmRequest {
    #[serde(default)]
    pub password: String,
    /// A one-time code or a recovery code, when the account has an
    /// authenticator.
    #[serde(default)]
    pub code: Option<String>,
}

/// `POST /api/auth/confirm` — 204 and a ten-minute mark on success.
pub async fn confirm(
    State(state): State<Arc<WebState>>,
    ClientIp(source): ClientIp,
    Extension(AuthedUser(address)): Extension<AuthedUser>,
    session: Option<Extension<SessionRef>>,
    axum::Json(req): axum::Json<ConfirmRequest>,
) -> Response {
    let Some(Extension(SessionRef(session))) = session else {
        return Refusal::AgentKeyNotAllowed.into_response();
    };
    let guard = &*crate::login_guard::GUARD;
    let event =
        |credential, result| LoginEvent::new(Channel::Web, credential, source, &address, result);

    let verdict = guard.check(source, &address).await;
    if let Verdict::Locked { .. } = verdict {
        guard
            .record(event(Credential::Password, Outcome::Locked))
            .await;
        return crate::login_guard::locked_response(verdict);
    }
    let failed_as =
        match crate::handlers::auth::password::verify(&state, &address, &req.password).await {
            Err(status) => return status.into_response(),
            Ok(None) => Some((Credential::Password, Outcome::BadPassword)),
            Ok(Some(_)) => match second_factor(&address, req.code.as_deref()) {
                Err(status) => return status.into_response(),
                Ok(SecondFactor::NotEnrolled | SecondFactor::Passed) => None,
                // the password was right, so this is not a guess: ask for the code
                Ok(SecondFactor::Missing) => {
                    return unauthorized(serde_json::json!({ "error": "totp_required" }));
                }
                Ok(SecondFactor::Failed) => Some((Credential::Totp, Outcome::MfaFailed)),
            },
        };

    if let Some((credential, outcome)) = failed_as {
        guard.failed(source, &address).await;
        guard.record(event(credential, outcome)).await;
        return failed(source, &address, &session).await;
    }

    guard.succeeded(source, &address).await;
    guard.record(event(Credential::Password, Outcome::Ok)).await;
    let (mark, failures) = (confirmed_key(&session), confirm_failures_key(&session));
    match with_kevy(move |c| {
        c.set_with_ttl(mark.as_bytes(), b"1", CONFIRM_TTL)?;
        c.del(&[failures.as_bytes()])?;
        Ok(())
    }) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(status) => status.into_response(),
    }
}

/// `GET /api/auth/confirm` — whether this session has a confirmation and
/// for how many more seconds, so a page asks for the password only when it
/// has to. Reads; changes nothing.
pub async fn status(session: Option<Extension<SessionRef>>) -> Response {
    let Some(Extension(SessionRef(session))) = session else {
        return Refusal::AgentKeyNotAllowed.into_response();
    };
    let key = confirmed_key(&session);
    match with_kevy(move |c| Ok(c.ttl_ms(key.as_bytes())?)) {
        // a key with no expiry is never written; a missing one reads below zero
        Ok(ms) if ms > 0 => axum::Json(serde_json::json!({
            "confirmed": true,
            "expires_in": (ms + 999) / 1000,
        }))
        .into_response(),
        Ok(_) => axum::Json(serde_json::json!({ "confirmed": false, "expires_in": null }))
            .into_response(),
        Err(status) => status.into_response(),
    }
}

/// Count one failure against the session, and end it at the limit.
async fn failed(source: std::net::IpAddr, address: &str, session: &str) -> Response {
    let key = confirm_failures_key(session);
    let count = match with_kevy(move |c| {
        let n = c.incr(key.as_bytes())?;
        c.expire(
            key.as_bytes(),
            std::time::Duration::from_secs(crate::session::SESSION_TTL_SECS),
        )?;
        Ok(n)
    }) {
        Ok(n) => n,
        Err(status) => return status.into_response(),
    };
    if count < FAILURES_TO_END {
        return unauthorized(serde_json::json!({
            "error": "confirm_failed",
            "remaining": FAILURES_TO_END - count,
        }));
    }
    let (addr, s) = (address.to_string(), session.to_string());
    if let Err(status) = with_kevy(move |c| end_session_by_ref(c, &addr, &s)) {
        return status.into_response();
    }
    let alert = mailrs_auth_guard::alerts::Alert {
        kind: mailrs_auth_guard::alerts::Kind::SessionEndedAfterFailedConfirmations,
        subject: address.to_string(),
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        detail: format!(
            "session {}…, source {source}, {count} failed confirmations; the session was ended",
            &session[..8]
        ),
        facts: vec![mailrs_auth_guard::alerts::Fact::Session {
            prefix: session[..8].to_string(),
            source: source.to_string(),
            failures: count.max(0) as u64,
        }],
    };
    if let Err(e) = crate::login_guard::GUARD.raise(alert).await {
        tracing::error!(error = %e, "confirm: the alert could not be queued");
    }
    tracing::warn!(account = %address, session = &session[..8], %source, "confirm: session ended after failed confirmations");
    unauthorized(serde_json::json!({ "error": "session_revoked" }))
}

/// End the session stored under `session` (its hash) and everything kept
/// beside it.
pub fn end_session_by_ref(
    c: &mut kevy_client::Connection,
    address: &str,
    session: &str,
) -> std::io::Result<()> {
    c.del(&[
        session_key_by_ref(session).as_bytes(),
        confirmed_key(session).as_bytes(),
        confirm_failures_key(session).as_bytes(),
    ])?;
    c.srem(by_addr_key(address).as_bytes(), &[session.as_bytes()])?;
    Ok(())
}
