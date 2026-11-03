//! A send held until later: when it was asked to go out, and what was done
//! to it while it waited.
//!
//! ```text
//! mailrs:send:{user}:{send_id}           field send_at — unix seconds
//! mailrs:send:{user}:{send_id}:events    list — JSON SendEvent, appended
//! ```
//!
//! `send_at` is written only for a send scheduled for the future: a send
//! that went at once has none, and its `created_at` is when it went.
//!
//! The events are facts, appended and never rewritten: `scheduled` when a
//! send is held, `cancelled` when it is stopped, each with who and when. A
//! cancel changes the send's status (a derivation) and adds an event; it
//! does not edit or remove the record that it was scheduled. Deleting the
//! send deletes its events with it.

use serde::{Deserialize, Serialize};

use super::send_key;

/// Record that `send_id` goes out at `at`.
pub fn set_send_at(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
    at: i64,
) -> std::io::Result<()> {
    conn.hset(
        send_key(user, send_id).as_bytes(),
        &[(b"send_at" as &[u8], at.to_string().as_bytes())],
    )?;
    Ok(())
}

/// When `send_id` was held until, if it was held.
pub fn send_at(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<Option<i64>> {
    let v = conn.hget(send_key(user, send_id).as_bytes(), b"send_at")?;
    Ok(v.and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.parse().ok()))
}

pub fn events_key(user: &str, send_id: &str) -> String {
    format!("mailrs:send:{user}:{send_id}:events")
}

/// Something done to a send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendEvent {
    /// `scheduled` or `cancelled`.
    pub kind: String,
    /// Unix seconds it happened.
    pub at: i64,
    /// Who did it: `web`, `jmap`, or `key <prefix>`.
    pub by: String,
    /// For `scheduled`: when it is to go out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_at: Option<i64>,
}

/// Append `e` to `send_id`'s events.
pub fn record_event(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
    e: &SendEvent,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(e).map_err(std::io::Error::other)?;
    conn.rpush(events_key(user, send_id).as_bytes(), &[bytes.as_slice()])?;
    Ok(())
}

/// `send_id`'s events, oldest first.
pub fn events(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<Vec<SendEvent>> {
    Ok(conn
        .lrange(events_key(user, send_id).as_bytes(), 0, -1)?
        .iter()
        .filter_map(|b| serde_json::from_slice(b).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_send_says_when_it_goes_and_another_says_nothing() {
        let mut c = kevy_client::Connection::connect("mem://send-release-at").unwrap();
        set_send_at(&mut c, "u", "m1@x", 1_800_000_000).unwrap();
        assert_eq!(send_at(&mut c, "u", "m1@x").unwrap(), Some(1_800_000_000));
        assert_eq!(send_at(&mut c, "u", "m2@x").unwrap(), None);
    }

    #[test]
    fn events_are_appended_in_order_and_kept() {
        let mut c = kevy_client::Connection::connect("mem://send-release-events").unwrap();
        let ev = |kind: &str, at, send_at| SendEvent {
            kind: kind.into(),
            at,
            by: "jmap".into(),
            send_at,
        };
        record_event(&mut c, "u", "m1@x", &ev("scheduled", 10, Some(100))).unwrap();
        record_event(&mut c, "u", "m1@x", &ev("cancelled", 20, None)).unwrap();
        assert_eq!(
            events(&mut c, "u", "m1@x").unwrap(),
            [ev("scheduled", 10, Some(100)), ev("cancelled", 20, None)]
        );
        assert!(events(&mut c, "u", "m2@x").unwrap().is_empty());
    }
}
