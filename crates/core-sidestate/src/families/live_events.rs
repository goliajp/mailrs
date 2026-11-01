//! Frames for the web clients' live stream, beyond new mail: the webapi's
//! feed consumers forward every SET under `keys::events::NOTIFY_PREFIX` to
//! the sockets of the user it names.
//!
//! Each frame expires after five minutes, which keeps the store bounded;
//! a client away longer refetches everything when its socket reconnects.

use crate::keys::events::notify_key;

/// The `MailboxChanged` frame for `user`: something in their mailbox was
/// read, moved or removed, from any client. It names no thread; the
/// reader refetches what it shows.
pub fn mailbox_changed_frame(user: &str) -> Vec<u8> {
    serde_json::json!({ "type": "MailboxChanged", "user": user.to_ascii_lowercase() })
        .to_string()
        .into_bytes()
}

/// Write the `MailboxChanged` frame for `user`.
pub fn mailbox_changed(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> kevy_client::KevyResult<()> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // the nanosecond names the frame; two users in one tick need two
    let key = format!("{}:{}", notify_key(nanos), user.to_ascii_lowercase());
    let frame = mailbox_changed_frame(user);
    conn.pipeline(|p| {
        p.cmd(&[b"SET", key.as_bytes(), frame.as_slice()]);
        p.cmd(&[b"EXPIRE", key.as_bytes(), b"300"]);
    })
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_names_its_user_lowercased_and_nothing_else() {
        let v: serde_json::Value =
            serde_json::from_slice(&mailbox_changed_frame("Bob@Golia.jp")).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "type": "MailboxChanged", "user": "bob@golia.jp" })
        );
    }
}
