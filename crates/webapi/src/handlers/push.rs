//! Device tokens for push notification delivery.
//!
//! The iOS app registers its APNs token here after sign-in; the sender
//! side lives in fastcore's spool drain, which reads the same hash when
//! a message lands and prunes any token Apple reports dead. This handler
//! is deliberately dumb storage: what "should be pushed" is decided
//! where mail arrives, not where tokens are filed.
//!
//! Fastcore-lane only (`.claude/rest-parity-allow.txt`): the monolith has
//! no push pipeline, and a registration endpoint whose tokens nothing
//! ever reads would be worse than a 404.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// One user's tokens: hash `push:tokens:{user}`, field = the token,
/// value = metadata. A hash rather than a set so re-registering the same
/// token (every app launch does) is one overwrite instead of a
/// read-modify-write.
pub(crate) fn tokens_key(user: &str) -> String {
    format!("push:tokens:{user}")
}

#[derive(Debug, Deserialize)]
pub struct RegisterPushTokenRequest {
    pub token: String,
    /// `"ios"` today. Stored so an Android registration later does not
    /// need a schema change, not because anything branches on it yet.
    #[serde(default)]
    pub platform: String,
}

/// POST /api/push/tokens
pub async fn register_push_token(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<RegisterPushTokenRequest>,
) -> Result<StatusCode, StatusCode> {
    // An APNs token is hex from the OS; anything else is a caller bug,
    // and storing it would make the sender loop over garbage forever.
    if req.token.is_empty() || req.token.len() > 512 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let key = tokens_key(&user);
    let meta = serde_json::json!({
        "platform": if req.platform.is_empty() { "ios" } else { &req.platform },
        "registered_at": now_secs(),
    })
    .to_string();
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(req.token.as_bytes(), meta.as_bytes())])
            .map_err(std::io::Error::other)
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/push/tokens/{token} — sign-out unregisters the device.
pub async fn delete_push_token(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(token): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let key = tokens_key(&user);
    with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[token.as_bytes()])
            .map_err(std::io::Error::other)
    })?;
    Ok(StatusCode::NO_CONTENT)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture in `wire-contract/requests/` is what the iOS client
    /// sends; this keeps the struct honest against it the same way the
    /// web client's bodies are pinned.
    #[test]
    fn ios_registration_body_parses() {
        let fixture = std::fs::read_to_string(format!(
            "{}/../../wire-contract/requests/push-register.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("fixture");
        let req: RegisterPushTokenRequest = serde_json::from_str(&fixture).expect("parse");
        assert!(!req.token.is_empty());
        assert_eq!(req.platform, "ios");
    }

    #[test]
    fn key_is_per_user() {
        assert_eq!(tokens_key("a@golia.jp"), "push:tokens:a@golia.jp");
    }
}
