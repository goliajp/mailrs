//! Per-user sender allow / block lists.
//!
//! ```text
//! spam:{user}:whitelist   set of lowercased sender addresses ("not junk")
//! spam:{user}:blacklist   set of lowercased sender addresses ("junk")
//! ```
//!
//! Written by the webapi (mark junk / not junk, the settings screen),
//! read by the receiver on every inbound message and by the fraud
//! sweep before it moves a conversation to Junk.

/// Key of `user`'s whitelist.
#[must_use]
pub fn whitelist_key(user: &str) -> String {
    format!("spam:{}:whitelist", user.to_lowercase())
}

/// Key of `user`'s blacklist.
#[must_use]
pub fn blacklist_key(user: &str) -> String {
    format!("spam:{}:blacklist", user.to_lowercase())
}

/// Every address on `user`'s whitelist, lowercased.
///
/// # Errors
/// The store could not be read — distinct from an empty list.
pub fn whitelist(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<std::collections::HashSet<String>> {
    let members = conn.smembers(whitelist_key(user).as_bytes())?;
    Ok(members
        .iter()
        .filter_map(|m| std::str::from_utf8(m).ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_reads_the_key_the_writers_use_lowercased() {
        let mut conn = kevy_client::Connection::connect("mem://sender-lists-whitelist").unwrap();
        conn.sadd(
            b"spam:u@example.com:whitelist",
            &[b"Friend@GOLIA.jp".as_slice()],
        )
        .unwrap();
        let list = whitelist(&mut conn, "U@Example.com").unwrap();
        assert_eq!(list.len(), 1);
        assert!(list.contains("friend@golia.jp"));
        assert!(
            whitelist(&mut conn, "other@example.com")
                .unwrap()
                .is_empty()
        );
    }
}
