//! Browser sessions.
//!
//! A session is stored under the sha256 of its token, never the token
//! itself: a key name shows up in every SCAN, and a key that is the token
//! signs whoever reads it in. The token is 32 random bytes, so a plain
//! digest cannot be reversed by guessing and a slow hash would only slow
//! down every request. The `h:` segment keeps these apart from the keys
//! written before 2026-10-30, which are the bare token and look the same
//! length.

/// What every session blob key starts with.
pub const SESSION_PREFIX: &str = "session:";

/// The name a session goes by in storage, indexes and logs.
pub fn session_ref(token: &str) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One session blob, by token.
pub fn session_key(token: &str) -> String {
    session_key_by_ref(&session_ref(token))
}

/// One session blob, by the hash an index holds.
pub fn session_key_by_ref(session_ref: &str) -> String {
    format!("session:h:{session_ref}")
}

/// Where a session issued before 2026-10-30 still lives: under the token.
/// Read once, moved to [`session_key`]. Sessions last seven days from their
/// last use, so nothing is left here a week after the release that moves
/// them, and this can go then.
pub fn legacy_session_key(token: &str) -> String {
    format!("session:{token}")
}

/// The sessions one address holds, by [`session_ref`], so they can all be
/// revoked. Entries written before 2026-10-30 are the token instead.
pub fn by_addr_key(address: &str) -> String {
    format!("session:by_addr:{address}")
}

/// Marks a session whose holder typed the password again within the last
/// few minutes; the operations a stolen session could use to stay in, or
/// to carry mail away, ask for it.
pub fn confirmed_key(session_ref: &str) -> String {
    format!("session:confirmed:{session_ref}")
}

/// Failed confirmations on one session; enough of them end the session.
pub fn confirm_failures_key(session_ref: &str) -> String {
    format!("session:confirm_failures:{session_ref}")
}

/// Every live token; the single-process server restores from it at boot.
pub const SESSION_INDEX: &[u8] = b"session:index";

/// Record `token` in `address`'s session index, so a password change or
/// account deletion can end it, and drop the tokens whose session has
/// expired: the index has no TTL of its own and would otherwise grow by
/// one entry per login forever.
pub fn index_session(
    c: &mut kevy_client::Connection,
    address: &str,
    token: &str,
) -> std::io::Result<()> {
    let idx = by_addr_key(address);
    for entry in c.smembers(idx.as_bytes())? {
        let [stored, legacy, moved] = keys_of_entry(&entry);
        if c.exists(&[stored.as_bytes(), legacy.as_bytes(), moved.as_bytes()])? == 0 {
            c.srem(idx.as_bytes(), &[entry.as_slice()])?;
        }
    }
    c.sadd(idx.as_bytes(), &[session_ref(token).as_bytes()])?;
    Ok(())
}

/// Every place an index entry's session can be. The entry is a hash; or,
/// for a session issued before hashing, the token, whose session is under
/// the token until its first use and under the token's hash after it.
fn keys_of_entry(entry: &[u8]) -> [String; 3] {
    let entry = String::from_utf8_lossy(entry);
    [
        session_key_by_ref(&entry),
        legacy_session_key(&entry),
        session_key(&entry),
    ]
}

/// Delete every session `address` holds. Returns how many tokens the
/// per-address index listed.
pub fn revoke_sessions(c: &mut kevy_client::Connection, address: &str) -> std::io::Result<usize> {
    let idx = by_addr_key(address);
    let entries = c.smembers(idx.as_bytes())?;
    for entry in &entries {
        let [stored, legacy, moved] = keys_of_entry(entry);
        c.del(&[stored.as_bytes(), legacy.as_bytes(), moved.as_bytes()])?;
        // a confirmation outlives nothing it was made on
        let entry = String::from_utf8_lossy(entry);
        for session in [entry.to_string(), session_ref(&entry)] {
            c.del(&[
                confirmed_key(&session).as_bytes(),
                confirm_failures_key(&session).as_bytes(),
            ])?;
        }
    }
    c.del(&[idx.as_bytes()])?;
    Ok(entries.len())
}

/// Every live session `address` holds: its ref, and its stored record.
///
/// # Errors
/// The store could not be read.
pub fn list_sessions(
    c: &mut kevy_client::Connection,
    address: &str,
) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    for entry in c.smembers(by_addr_key(address).as_bytes())? {
        let keys = keys_of_entry(&entry);
        let Some(blob) = keys.iter().find_map(|k| c.get(k.as_bytes()).ok().flatten()) else {
            continue;
        };
        out.push((ref_of_entry(&entry, &keys[0], c)?, blob));
    }
    Ok(out)
}

/// The ref an index entry stands for: the entry itself, or the hash of the
/// token an entry from before hashing is.
fn ref_of_entry(
    entry: &[u8],
    stored: &str,
    c: &mut kevy_client::Connection,
) -> std::io::Result<String> {
    let entry = String::from_utf8_lossy(entry).to_string();
    match c.exists(&[stored.as_bytes()])? {
        0 => Ok(session_ref(&entry)),
        _ => Ok(entry),
    }
}

/// End the one session of `address`'s whose ref is `reference`. Returns
/// whether `address` held it.
///
/// # Errors
/// The store could not be read or written.
pub fn end_session_by_ref(
    c: &mut kevy_client::Connection,
    address: &str,
    reference: &str,
) -> std::io::Result<bool> {
    let idx = by_addr_key(address);
    for entry in c.smembers(idx.as_bytes())? {
        let text = String::from_utf8_lossy(&entry).to_string();
        if text != reference && session_ref(&text) != reference {
            continue;
        }
        let [stored, legacy, moved] = keys_of_entry(&entry);
        c.del(&[stored.as_bytes(), legacy.as_bytes(), moved.as_bytes()])?;
        c.del(&[
            confirmed_key(reference).as_bytes(),
            confirm_failures_key(reference).as_bytes(),
        ])?;
        c.srem(idx.as_bytes(), &[entry.as_slice()])?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keys_keep_their_prod_spelling() {
        assert_eq!(SESSION_PREFIX, "session:");
        assert_eq!(legacy_session_key("t0k"), "session:t0k");
        assert_eq!(by_addr_key("a@x.jp"), "session:by_addr:a@x.jp");
        assert_eq!(SESSION_INDEX, b"session:index");
        assert_eq!(confirmed_key("r"), "session:confirmed:r");
        assert_eq!(confirm_failures_key("r"), "session:confirm_failures:r");
    }

    #[test]
    fn one_session_is_listed_and_ended_by_its_ref() {
        let mut c = kevy_client::Connection::connect("mem://session-list-end").unwrap();
        let (a, b) = ("aa".repeat(32), "bb".repeat(32));
        for t in [&a, &b] {
            c.set(session_key(t).as_bytes(), b"{}").unwrap();
            index_session(&mut c, "u@x.test", t).unwrap();
        }
        let mut refs: Vec<String> = list_sessions(&mut c, "u@x.test")
            .unwrap()
            .into_iter()
            .map(|(r, _)| r)
            .collect();
        refs.sort();
        let mut want = vec![session_ref(&a), session_ref(&b)];
        want.sort();
        assert_eq!(refs, want);
        assert!(!end_session_by_ref(&mut c, "other@x.test", &session_ref(&a)).unwrap());
        assert!(end_session_by_ref(&mut c, "u@x.test", &session_ref(&a)).unwrap());
        assert_eq!(c.get(session_key(&a).as_bytes()).unwrap(), None);
        assert!(c.get(session_key(&b).as_bytes()).unwrap().is_some());
        assert_eq!(list_sessions(&mut c, "u@x.test").unwrap().len(), 1);
    }

    #[test]
    fn the_stored_key_does_not_contain_the_token() {
        assert_eq!(
            session_key("abc"),
            "session:h:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let token = "ab".repeat(32);
        assert!(!session_key(&token).contains(&token));
    }
}
