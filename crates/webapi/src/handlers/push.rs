//! Device tokens for push notification delivery.
//!
//! The iOS app registers its APNs token here after sign-in; the sender
//! side lives in fastcore's spool drain, which reads the same hash when
//! a message lands and prunes any token Apple reports dead. This handler
//! only files the token, through the account-wide device cap
//! (`push_devices::register`): what "should be pushed" is decided where
//! mail arrives, not where tokens are filed.
//!
//! A token a person removed from another device is refused with
//! `410 {"error":"removed"}` unless the request says the person turned
//! pushes on again (`resume`), since the app registers on every launch.
//!
//! Fastcore only, and permanently: the monolith is not coming back.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::families::push_devices::{self, Asker, Channel, Filed, Registration};
use serde::Deserialize;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Deserialize)]
pub struct RegisterPushTokenRequest {
    pub token: String,
    /// `"ios"` today. Stored so an Android registration later does not
    /// need a schema change, not because anything branches on it yet.
    #[serde(default)]
    pub platform: String,
    /// The person turned pushes on on this device just now.
    #[serde(default)]
    pub resume: bool,
}

/// The ref of the session a request came with; none for an agent key,
/// which belongs to no device.
pub(crate) fn session_of(headers: &HeaderMap) -> Option<String> {
    crate::session::session_token(headers)
        .filter(|t| !t.starts_with(crate::session::AGENT_KEY_PREFIX))
        .map(|t| mailrs_core_sidestate::keys::session::session_ref(&t))
}

/// The answer to a device a person removed from another device.
pub(crate) fn removed_answer() -> Response {
    (
        StatusCode::GONE,
        Json(serde_json::json!({ "error": "removed" })),
    )
        .into_response()
}

/// Register through the cap, linked to the asking session.
pub(crate) fn file(
    c: &mut kevy_client::Connection,
    user: &str,
    reg: Registration<'_>,
    session: Option<&str>,
    resume: bool,
    now: i64,
) -> std::io::Result<Filed> {
    let filed = push_devices::register(c, user, reg, Asker { session, resume }, now)?;
    if let Filed::Registered(done) = &filed {
        log_evicted(user, &done.evicted);
    }
    Ok(filed)
}

/// POST /api/push/tokens
pub async fn register_push_token(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    headers: HeaderMap,
    Json(req): Json<RegisterPushTokenRequest>,
) -> Result<Response, StatusCode> {
    // An APNs token is hex from the OS; anything else is a caller bug,
    // and storing it would make the sender loop over garbage forever.
    if req.token.is_empty() || req.token.len() > 512 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let platform = match req.platform.is_empty() {
        true => "ios".to_string(),
        false => req.platform,
    };
    let session = session_of(&headers);
    let now = now_secs();
    let filed = with_kevy(move |c| {
        let reg = Registration::Apns {
            token: &req.token,
            platform: &platform,
        };
        file(c, &user, reg, session.as_deref(), req.resume, now)
    })?;
    match filed {
        Filed::Registered(_) => Ok(StatusCode::NO_CONTENT.into_response()),
        Filed::Removed => Ok(removed_answer()),
    }
}

/// Say which devices a registration pushed out; by channel and count only,
/// since a device id can be a push token.
pub(crate) fn log_evicted(user: &str, evicted: &[(Channel, String)]) {
    for (channel, _) in evicted {
        tracing::info!(%user, channel = channel.as_str(), "push: device cap reached, least recently used device removed");
    }
}

/// DELETE /api/push/tokens/{token} — stop pushes to that device. From
/// another device's session, the device is kept from registering back.
pub async fn delete_push_token(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let session = session_of(&headers);
    let now = now_secs();
    with_kevy(move |c| {
        push_devices::remove(c, &user, Channel::Apns, &token, session.as_deref(), now)
    })?;
    Ok(StatusCode::NO_CONTENT)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
