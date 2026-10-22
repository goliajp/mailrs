//! Browser sessions. The webapi and the single-process server write the
//! same shape, so a token issued by one is honoured by the other.

/// What every session blob key starts with.
pub const SESSION_PREFIX: &str = "session:";

/// One session blob.
pub fn session_key(token: impl std::fmt::Display) -> String {
    format!("session:{token}")
}

/// The set of tokens one address holds, so they can all be revoked.
pub fn by_addr_key(address: &str) -> String {
    format!("session:by_addr:{address}")
}

/// Every live token; the single-process server restores from it at boot.
pub const SESSION_INDEX: &[u8] = b"session:index";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keys_keep_their_prod_spelling() {
        assert_eq!(SESSION_PREFIX, "session:");
        assert_eq!(session_key("t0k"), "session:t0k");
        assert_eq!(by_addr_key("a@x.jp"), "session:by_addr:a@x.jp");
        assert_eq!(SESSION_INDEX, b"session:index");
    }
}
