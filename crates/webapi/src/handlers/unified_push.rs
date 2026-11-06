//! A reader's Android phones on UnifiedPush.
//!
//! - `GET /api/push/unifiedpush` — `{enabled, vapid_public_key, devices}`;
//!   `enabled: false` when no VAPID key is configured, since the pushes are
//!   signed with it. A device is listed by id, name and its distributor's
//!   host, never by its endpoint: whoever holds that can push to the phone
//! - `POST /api/push/unifiedpush` — `{endpoint, keys: {p256dh, auth}, name}`
//!   from the phone; answers the device's `id`
//! - `DELETE /api/push/unifiedpush/{id}`
//!
//! What a push says, and when one is sent, is `mailrs_intake::unified_push`.

use axum::Json;
use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::push_devices;
use mailrs_core_sidestate::families::unified_push::{self, Device};

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::web_push::{Keys, vapid};

#[derive(Debug, serde::Serialize)]
pub struct ListedDevice {
    pub id: String,
    pub name: String,
    /// The distributor's server, so a reader can tell which app carries it.
    pub host: String,
    pub created_at: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct State {
    pub enabled: bool,
    /// The VAPID public key a phone registers with its distributor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vapid_public_key: Option<String>,
    pub devices: Vec<ListedDevice>,
}

fn listed((id, d): (String, Device)) -> ListedDevice {
    let host = url::Url::parse(&d.endpoint)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    ListedDevice {
        id,
        name: d.name,
        host,
        created_at: d.created_at,
    }
}

/// GET /api/push/unifiedpush
pub async fn state(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<State>, StatusCode> {
    let devices = with_kevy(move |c| unified_push::list(c, &user))?;
    let vapid_public_key = vapid().map(|v| v.public_key());
    Ok(Json(State {
        enabled: vapid_public_key.is_some(),
        vapid_public_key,
        devices: devices.into_iter().map(listed).collect(),
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct RegisterBody {
    pub endpoint: String,
    pub keys: Keys,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, serde::Serialize)]
pub struct Registered {
    pub id: String,
}

/// The longest device name kept.
const NAME_MAX: usize = 64;

/// Whether a registration has the shape one that can be pushed to has: an
/// https endpoint on 443 naming a host, a P-256 public key, a 16-byte auth
/// secret and a short name. The host's addresses are checked by
/// [`register`].
pub fn well_formed(body: &RegisterBody) -> bool {
    let endpoint_ok = body.endpoint.len() <= 2048
        && url::Url::parse(&body.endpoint)
            .ok()
            .is_some_and(|u| mailrs_webpush::endpoint::distributor_host(&u).is_ok());
    let key_ok = mailrs_webpush::from_b64url(&body.keys.p256dh)
        .is_some_and(|k| k.len() == 65 && k[0] == 0x04);
    let auth_ok = mailrs_webpush::from_b64url(&body.keys.auth).is_some_and(|a| a.len() == 16);
    let name_ok = body.name.chars().count() <= NAME_MAX;
    endpoint_ok && key_ok && auth_ok && name_ok
}

/// Whether every address the endpoint's host resolves to is public. Checked
/// again on every push, since a name can change its answer.
async fn public(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    match mailrs_webpush::endpoint::resolve_distributor(&url).await {
        Ok(_) => true,
        Err(e) => {
            tracing::info!(error = %e, "unifiedpush: endpoint refused");
            false
        }
    }
}

/// POST /api/push/unifiedpush
pub async fn register(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(body): Json<RegisterBody>,
) -> Result<Json<Registered>, StatusCode> {
    if vapid().is_none() {
        return Err(StatusCode::CONFLICT);
    }
    if !well_formed(&body) || !public(&body.endpoint).await {
        return Err(StatusCode::BAD_REQUEST);
    }
    let now = chrono::Utc::now().timestamp();
    let device = Device {
        endpoint: body.endpoint,
        p256dh: body.keys.p256dh,
        auth: body.keys.auth,
        name: body.name,
        created_at: now,
    };
    let id = with_kevy(move |c| {
        let reg = push_devices::Registration::UnifiedPush(&device);
        let done = push_devices::register(c, &user, reg, now)?;
        crate::handlers::push::log_evicted(&user, &done.evicted);
        Ok(done.id)
    })?;
    Ok(Json(Registered { id }))
}

/// DELETE /api/push/unifiedpush/{id}
pub async fn unregister(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| unified_push::remove(c, &user, &id))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(endpoint: &str) -> RegisterBody {
        let mut key = vec![0x04];
        key.extend([7u8; 64]);
        RegisterBody {
            endpoint: endpoint.into(),
            keys: Keys {
                p256dh: mailrs_webpush::b64url(&key),
                auth: mailrs_webpush::b64url(&[1u8; 16]),
            },
            name: "Pixel 8".into(),
        }
    }

    #[test]
    fn any_https_host_name_with_real_keys_is_well_formed() {
        assert!(well_formed(&body("https://ntfy.example.org/upAbC123?up=1")));
        assert!(!well_formed(&body("http://ntfy.example.org/upAbC123")));
        assert!(!well_formed(&body("https://10.0.0.7/upAbC123")));
        assert!(!well_formed(&body("https://[fd00::1]/upAbC123")));
        assert!(!well_formed(&body("https://ntfy.example.org:8443/up1")));
        let mut short_key = body("https://ntfy.example.org/up1");
        short_key.keys.p256dh = mailrs_webpush::b64url(&[4u8; 33]);
        assert!(!well_formed(&short_key));
        let mut long_name = body("https://ntfy.example.org/up1");
        long_name.name = "x".repeat(NAME_MAX + 1);
        assert!(!well_formed(&long_name));
    }

    #[tokio::test]
    async fn a_name_that_resolves_to_loopback_is_refused() {
        assert!(well_formed(&body("https://localhost/up1")));
        assert!(!public("https://localhost/up1").await);
    }

    #[test]
    fn a_device_is_listed_by_its_host_not_its_endpoint() {
        let shown = listed((
            "abc".into(),
            Device {
                endpoint: "https://ntfy.example.org/upSECRET".into(),
                p256dh: "k".into(),
                auth: "a".into(),
                name: "Pixel".into(),
                created_at: 5,
            },
        ));
        assert_eq!(shown.host, "ntfy.example.org");
        let json = serde_json::to_string(&shown).unwrap();
        assert!(!json.contains("SECRET"));
    }
}
