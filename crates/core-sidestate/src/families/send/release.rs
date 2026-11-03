//! When a send was asked to go out, for a send held until later.
//!
//! A field on the send's own row (`send_at`, unix seconds), written only
//! for a send scheduled for the future: a send that went at once has none,
//! and its `created_at` is when it went. Deleting the row deletes it.

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
}
