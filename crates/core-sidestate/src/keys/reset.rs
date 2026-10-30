//! A code to set a new password on an account whose password an
//! administrator cleared. One hash per account; issuing again replaces it,
//! and the key's expiry is the code's.

/// `reset:code:<address>`: `mac` (HMAC-SHA256 of the code, hex), `key_id`
/// (which key made it), `issued_by`, `issued_at`, `expires_at`.
pub fn code_key(address: &str) -> String {
    format!("reset:code:{}", address.to_ascii_lowercase())
}

/// `reset:failures:<address>`: wrong tries at the code out, a counter of
/// its own so concurrent tries are all counted; it expires with the code.
pub fn failures_key(address: &str) -> String {
    format!("reset:failures:{}", address.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_key_keeps_its_spelling_and_ignores_case() {
        assert_eq!(super::code_key("Alice@X.test"), "reset:code:alice@x.test");
        assert_eq!(
            super::failures_key("Alice@X.test"),
            "reset:failures:alice@x.test"
        );
    }
}
