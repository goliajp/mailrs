//! Frames for the web clients' live stream, beyond new mail, published on
//! `keys::events::EVENTS_CHANNEL`.

/// The `MailboxChanged` frame for `user`: something in their mailbox was
/// read, moved or removed, from any client. It names no thread; the
/// reader refetches what it shows.
pub fn mailbox_changed_frame(user: &str) -> Vec<u8> {
    serde_json::json!({ "type": "MailboxChanged", "user": user.to_ascii_lowercase() })
        .to_string()
        .into_bytes()
}

/// Publish the `MailboxChanged` frame for `user`.
pub fn mailbox_changed(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> kevy_client::KevyResult<()> {
    crate::keys::events::send(conn, &mailbox_changed_frame(user))
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
