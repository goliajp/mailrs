//! A reader's phones on UnifiedPush: the endpoint each phone's distributor
//! gave it and the keys pushes to it are encrypted with.
//!
//! ```text
//! push:unifiedpush:{user}   hash  sha256(endpoint) -> Device JSON
//! ```
//!
//! The field is also the device's id on the wire, so a phone can drop its
//! own registration and a list never has to show the endpoint, which is a
//! capability URL: whoever holds it can push to the phone.

use serde::{Deserialize, Serialize};

use super::notify::endpoint_field;
use crate::keys::push::unified_push_key;

/// One phone's registration as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub endpoint: String,
    /// The phone's `p256dh` key, base64url.
    pub p256dh: String,
    /// The phone's `auth` secret, base64url.
    pub auth: String,
    /// What the phone calls itself, for the list.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub created_at: i64,
}

/// A device's id: the field it is filed under.
pub fn id_of(endpoint: &str) -> String {
    endpoint_field(endpoint)
}

/// Keep `device` for `user`; the same endpoint replaces its old keys. The
/// device's id.
pub fn add(
    conn: &mut kevy_client::Connection,
    user: &str,
    device: &Device,
) -> std::io::Result<String> {
    let body = serde_json::to_vec(device).map_err(std::io::Error::other)?;
    let id = id_of(&device.endpoint);
    conn.hset(
        unified_push_key(user).as_bytes(),
        &[(id.as_bytes(), body.as_slice())],
    )?;
    Ok(id)
}

/// Forget the device filed under `id`; whether it was there.
pub fn remove(conn: &mut kevy_client::Connection, user: &str, id: &str) -> std::io::Result<bool> {
    Ok(conn.hdel(unified_push_key(user).as_bytes(), &[id.as_bytes()])? > 0)
}

/// Every device `user` registered, with its id, oldest first.
pub fn list(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<Vec<(String, Device)>> {
    let mut out: Vec<(String, Device)> = conn
        .hgetall(unified_push_key(user).as_bytes())?
        .chunks_exact(2)
        .filter_map(|p| {
            let device: Device = serde_json::from_slice(&p[1]).ok()?;
            Some((String::from_utf8_lossy(&p[0]).into_owned(), device))
        })
        .collect();
    out.sort_by(|a, b| (a.1.created_at, &a.0).cmp(&(b.1.created_at, &b.0)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_is_kept_once_per_endpoint_and_dropped_by_id() {
        let mut c = kevy_client::Connection::connect("mem://unified-push").unwrap();
        let phone = Device {
            endpoint: "https://ntfy.example.org/upAAA".into(),
            p256dh: "k".into(),
            auth: "a".into(),
            name: "Pixel 8".into(),
            created_at: 2,
        };
        let tablet = Device {
            endpoint: "https://ntfy.example.org/upBBB".into(),
            name: "Tab".into(),
            created_at: 1,
            ..phone.clone()
        };
        let id = add(&mut c, "Me@x", &phone).unwrap();
        assert_eq!(id, id_of(&phone.endpoint));
        add(
            &mut c,
            "me@x",
            &Device {
                p256dh: "k2".into(),
                ..phone.clone()
            },
        )
        .unwrap();
        add(&mut c, "me@x", &tablet).unwrap();
        let listed = list(&mut c, "me@x").unwrap();
        assert_eq!(listed.len(), 2, "one endpoint, one device");
        assert_eq!(listed[0].1.name, "Tab", "oldest first");
        assert_eq!(listed[1].1.p256dh, "k2", "re-registering replaced the keys");
        assert!(remove(&mut c, "me@x", &id).unwrap());
        assert!(!remove(&mut c, "me@x", &id).unwrap());
        assert_eq!(list(&mut c, "me@x").unwrap().len(), 1);
    }
}
