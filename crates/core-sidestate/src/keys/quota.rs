//! Per-user mailbox quota. fastcore mirrors the limit and counts deliveries,
//! the webapi counts sent copies, the receiver enforces the limit.

/// Bytes the user's mailbox holds.
pub fn used_key(user: &str) -> String {
    format!("mailrs:quota:{}:used_bytes", user.to_lowercase())
}

/// The user's limit in bytes; absent or 0 means unlimited.
pub fn limit_key(user: &str) -> String {
    format!("mailrs:quota:{}:limit_bytes", user.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_keys_keep_their_prod_spelling() {
        assert_eq!(used_key("A@X.jp"), "mailrs:quota:a@x.jp:used_bytes");
        assert_eq!(limit_key("A@X.jp"), "mailrs:quota:a@x.jp:limit_bytes");
    }
}
