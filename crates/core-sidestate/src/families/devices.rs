//! The devices an account has signed in from, to tell its owner about a
//! new one.
//!
//! ```text
//! devices:{user}   hash  fingerprint → {"first_seen","user_agent"}
//! ```
//!
//! A device is its user agent with the version numbers taken out, so a
//! browser updating itself is not a new device.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    first_seen: i64,
    user_agent: String,
}

/// What a sign-in from this user agent is to the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// The account's first recorded sign-in: nothing to compare with.
    First,
    /// Seen before.
    Known,
    /// Not seen before, and the account has signed in from others.
    New,
}

/// Key of `user`'s devices.
#[must_use]
pub fn key(user: &str) -> String {
    format!("devices:{}", user.to_lowercase())
}

/// The fingerprint of a user agent: without its digits, lowercased.
pub fn fingerprint(user_agent: &str) -> String {
    let bare: String = user_agent
        .chars()
        .filter(|c| !c.is_ascii_digit())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Sha256::digest(bare.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Record a sign-in from `user_agent` and say what it was.
///
/// # Errors
/// The store could not be read or written.
pub fn sign_in(
    conn: &mut kevy_client::Connection,
    user: &str,
    user_agent: &str,
    now: i64,
) -> std::io::Result<Device> {
    let k = key(user);
    let print = fingerprint(user_agent);
    if conn.hget(k.as_bytes(), print.as_bytes())?.is_some() {
        return Ok(Device::Known);
    }
    let any = conn.hlen(k.as_bytes())? > 0;
    let seen = Seen {
        first_seen: now,
        user_agent: user_agent.chars().take(300).collect(),
    };
    let bytes = serde_json::to_vec(&seen).map_err(std::io::Error::other)?;
    conn.hset(k.as_bytes(), &[(print.as_bytes(), bytes.as_slice())])?;
    Ok(match any {
        true => Device::New,
        false => Device::First,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFARI_17: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15";
    const SAFARI_18: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15";
    const FIREFOX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0";

    #[test]
    fn a_browser_update_is_not_a_new_device() {
        assert_eq!(fingerprint(SAFARI_17), fingerprint(SAFARI_18));
        assert_ne!(fingerprint(SAFARI_17), fingerprint(FIREFOX));
    }

    #[test]
    fn the_first_sign_in_is_recorded_and_the_next_new_one_is_told() {
        let mut c = kevy_client::Connection::connect("mem://devices-sign-in").unwrap();
        assert_eq!(
            sign_in(&mut c, "U@x.test", SAFARI_17, 1).unwrap(),
            Device::First
        );
        assert_eq!(
            sign_in(&mut c, "u@x.test", SAFARI_18, 2).unwrap(),
            Device::Known
        );
        assert_eq!(
            sign_in(&mut c, "u@x.test", FIREFOX, 3).unwrap(),
            Device::New
        );
        assert_eq!(
            sign_in(&mut c, "u@x.test", FIREFOX, 4).unwrap(),
            Device::Known
        );
        assert_eq!(
            sign_in(&mut c, "other@x.test", FIREFOX, 5).unwrap(),
            Device::First
        );
    }
}
