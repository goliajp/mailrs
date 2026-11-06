//! Web Push subscriptions for this browser, and whether Web Push is on.
//!
//! - `GET /api/push/web` — `{enabled, public_key}`; `enabled: false` when
//!   no VAPID key is configured, so the settings page can say so
//! - `POST /api/push/web-subscriptions` — a browser's `PushSubscription`
//! - `DELETE /api/push/web-subscriptions` — `{endpoint}`
//! - `POST /api/push/clicked` — a notification was opened

use std::sync::OnceLock;

use axum::Json;
use axum::extract::Extension;
use axum::http::StatusCode;
use mailrs_core_sidestate::families::{notify, push_devices};
use mailrs_webpush::vapid::Vapid;

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

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
}

/// GET /api/push/web
pub async fn state() -> Json<WebPushState> {
    let public_key = vapid().map(Vapid::public_key);
    Json(WebPushState {
        enabled: public_key.is_some(),
        public_key,
    })
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
    Json(body): Json<SubscribeBody>,
) -> Result<StatusCode, StatusCode> {
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
    with_kevy(move |c| {
        let done = push_devices::register(c, &user, push_devices::Registration::Web(&sub), now)?;
        crate::handlers::push::log_evicted(&user, &done.evicted);
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
pub struct UnsubscribeBody {
    pub endpoint: String,
}

/// DELETE /api/push/web-subscriptions
pub async fn unsubscribe(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(body): Json<UnsubscribeBody>,
) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| notify::remove_web_subscription(c, &user, &body.endpoint))?;
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
