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
    read_set(conn, &whitelist_key(user))
}

/// Every address on `user`'s blacklist, lowercased.
///
/// # Errors
/// The store could not be read — distinct from an empty list.
pub fn blacklist(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<std::collections::HashSet<String>> {
    read_set(conn, &blacklist_key(user))
}

fn read_set(
    conn: &mut kevy_client::Connection,
    key: &str,
) -> std::io::Result<std::collections::HashSet<String>> {
    let members = conn.smembers(key.as_bytes())?;
    Ok(members
        .iter()
        .filter_map(|m| std::str::from_utf8(m).ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Put `address` on `user`'s blacklist and take it off the whitelist.
///
/// An address is on at most one of the two lists, and every writer goes
/// through this or [`allow`]: delivery checks the whitelist first, so an
/// address on both would never be blocked. Returns whether it was on the
/// whitelist, so a caller can say so and put it back.
///
/// # Errors
/// The store could not be written.
pub fn block(
    conn: &mut kevy_client::Connection,
    user: &str,
    address: &str,
) -> std::io::Result<bool> {
    let addr = address.trim().to_lowercase();
    let was_allowed = conn.srem(whitelist_key(user).as_bytes(), &[addr.as_bytes()])? > 0;
    conn.sadd(blacklist_key(user).as_bytes(), &[addr.as_bytes()])?;
    Ok(was_allowed)
}

/// Put `address` on `user`'s whitelist and take it off the blacklist.
/// Returns whether it was on the blacklist. See [`block`].
///
/// # Errors
/// The store could not be written.
pub fn allow(
    conn: &mut kevy_client::Connection,
    user: &str,
    address: &str,
) -> std::io::Result<bool> {
    let addr = address.trim().to_lowercase();
    let was_blocked = conn.srem(blacklist_key(user).as_bytes(), &[addr.as_bytes()])? > 0;
    conn.sadd(whitelist_key(user).as_bytes(), &[addr.as_bytes()])?;
    Ok(was_blocked)
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

    #[test]
    fn an_address_is_on_one_list_at_most() {
        let mut conn = kevy_client::Connection::connect("mem://sender-lists-one-list").unwrap();
        let user = "U@Example.com";
        assert!(!allow(&mut conn, user, "x@y.example").unwrap());
        assert!(block(&mut conn, user, " X@Y.example ").unwrap());
        assert!(whitelist(&mut conn, user).unwrap().is_empty());
        assert!(blacklist(&mut conn, user).unwrap().contains("x@y.example"));
        // a second block finds nothing on the whitelist
        assert!(!block(&mut conn, user, "x@y.example").unwrap());
        assert!(allow(&mut conn, user, "x@y.example").unwrap());
        assert!(blacklist(&mut conn, user).unwrap().is_empty());
        assert!(whitelist(&mut conn, user).unwrap().contains("x@y.example"));
    }
}
