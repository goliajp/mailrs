//! Web Push subscriptions for this browser, and whether Web Push is on.
//!
//! - `GET /api/push/web` — `{enabled, public_key, removed}`; `enabled:
//!   false` when no VAPID key is configured, so the settings page can say
//!   so; `removed` lists browsers a person removed from another device
//! - `POST /api/push/web-subscriptions` — a browser's `PushSubscription`,
//!   plus `resume` when the person turned pushes on just now; a removed
//!   browser without it is answered `410 {"error":"removed"}`
//! - `DELETE /api/push/web-subscriptions` — `{endpoint}`
//! - `POST /api/push/clicked` — a notification was opened

use std::sync::OnceLock;

use axum::Json;
use axum::extract::Extension;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::families::notify;
use mailrs_core_sidestate::families::push_devices::{self, Channel, Filed, Registration};
use mailrs_webpush::vapid::Vapid;

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::push;

/// The configured key, read once; `None` means Web Push is off.
pub(crate) fn vapid() -> Option<&'static Vapid> {
    static VAPID: OnceLock<Option<Vapid>> = OnceLock::new();
    VAPID
        .get_or_init(|| match Vapid::from_env() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "web push: key not usable — disabled");
                None
            }
        })
        .as_ref()
}

#[derive(Debug, serde::Serialize)]
pub struct WebPushState {
    pub enabled: bool,
    /// The `applicationServerKey` to subscribe with, when enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    /// Browsers a person removed from another device, by the sha256 of
    /// their endpoint, so a browser can tell it was one of them.
    pub removed: Vec<String>,
}

/// GET /api/push/web
pub async fn state(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<WebPushState>, StatusCode> {
    let removed = with_kevy(move |c| push_devices::removed(c, &user, Channel::Web))?;
    let public_key = vapid().map(Vapid::public_key);
    Ok(Json(WebPushState {
        enabled: public_key.is_some(),
        public_key,
        removed,
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct Keys {
    pub p256dh: String,
    pub auth: String,
}

/// The shape `PushSubscription.toJSON()` produces.
#[derive(Debug, serde::Deserialize)]
pub struct SubscribeBody {
    pub endpoint: String,
    pub keys: Keys,
    /// The person turned pushes on in this browser just now.
    #[serde(default)]
    pub resume: bool,
}

/// Whether a browser's subscription may be stored: a known push service,
/// a P-256 public key and a 16-byte auth secret.
pub fn acceptable(body: &SubscribeBody) -> bool {
    let endpoint_ok = body.endpoint.len() <= 2048
        && url::Url::parse(&body.endpoint)
            .ok()
            .is_some_and(|u| mailrs_webpush::endpoint::host(&u).is_ok());
    let key_ok = mailrs_webpush::from_b64url(&body.keys.p256dh)
        .is_some_and(|k| k.len() == 65 && k[0] == 0x04);
    let auth_ok = mailrs_webpush::from_b64url(&body.keys.auth).is_some_and(|a| a.len() == 16);
    endpoint_ok && key_ok && auth_ok
}

/// POST /api/push/web-subscriptions
pub async fn subscribe(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    headers: HeaderMap,
    Json(body): Json<SubscribeBody>,
) -> Result<Response, StatusCode> {
    if vapid().is_none() {
        return Err(StatusCode::CONFLICT);
    }
    if !acceptable(&body) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let sub = notify::WebSubscription {
        endpoint: body.endpoint,
        p256dh: body.keys.p256dh,
        auth: body.keys.auth,
        created_at: chrono::Utc::now().timestamp(),
    };
    let now = sub.created_at;
    let session = push::session_of(&headers);
    let filed = with_kevy(move |c| {
        let reg = Registration::Web(&sub);
        push::file(c, &user, reg, session.as_deref(), body.resume, now)
    })?;
    match filed {
        Filed::Registered(_) => Ok(StatusCode::NO_CONTENT.into_response()),
        Filed::Removed => Ok(push::removed_answer()),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct UnsubscribeBody {
    pub endpoint: String,
}

/// DELETE /api/push/web-subscriptions
pub async fn unsubscribe(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    headers: HeaderMap,
    Json(body): Json<UnsubscribeBody>,
) -> Result<StatusCode, StatusCode> {
    let session = push::session_of(&headers);
    let now = chrono::Utc::now().timestamp();
    let id = notify::endpoint_field(&body.endpoint);
    with_kevy(move |c| push_devices::remove(c, &user, Channel::Web, &id, session.as_deref(), now))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
pub struct ClickedBody {
    pub notification_id: String,
}

/// POST /api/push/clicked
pub async fn clicked(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(body): Json<ClickedBody>,
) -> Result<StatusCode, StatusCode> {
    if body.notification_id.is_empty() || body.notification_id.len() > 64 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let now = chrono::Utc::now().timestamp();
    with_kevy(move |c| notify::count(c, &user, "web", "clicked", now))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(endpoint: &str, p256dh: Vec<u8>, auth: Vec<u8>) -> SubscribeBody {
        SubscribeBody {
            endpoint: endpoint.into(),
            keys: Keys {
                p256dh: mailrs_webpush::b64url(&p256dh),
                auth: mailrs_webpush::b64url(&auth),
            },
            resume: false,
        }
    }

    #[test]
    fn only_known_push_services_with_real_keys_are_stored() {
        let mut key = vec![0x04];
        key.extend([7u8; 64]);
        let good = "https://fcm.googleapis.com/fcm/send/abc";
        assert!(acceptable(&body(good, key.clone(), vec![1; 16])));
        assert!(!acceptable(&body(
            "https://evil.test/push",
            key.clone(),
            vec![1; 16]
        )));
        assert!(!acceptable(&body(
            "http://fcm.googleapis.com/x",
            key.clone(),
            vec![1; 16]
        )));
        assert!(!acceptable(&body(good, vec![0x04; 33], vec![1; 16])));
        assert!(!acceptable(&body(good, key, vec![1; 8])));
    }
}
