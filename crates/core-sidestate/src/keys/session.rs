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
    fn the_stored_key_does_not_contain_the_token() {
        assert_eq!(
            session_key("abc"),
            "session:h:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let token = "ab".repeat(32);
        assert!(!session_key(&token).contains(&token));
    }
}
