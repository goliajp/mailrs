//! Mobile push tokens: the webapi registers them, intake reads them to
//! notify on new mail.

/// One user's tokens: hash, field = the token, value = metadata.
pub fn tokens_key(user: &str) -> String {
    format!("push:tokens:{user}")
}

/// One user's Web Push subscriptions: hash, field = the endpoint's
/// sha256, value = the subscription JSON.
pub fn web_subscriptions_key(user: &str) -> String {
    format!("push:web:{}", user.to_lowercase())
}

/// One user's phones on UnifiedPush: hash, field = the endpoint's sha256,
/// value = the device JSON.
pub fn unified_push_key(user: &str) -> String {
    format!("push:unifiedpush:{}", user.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_keys_keep_their_prod_spelling() {
        assert_eq!(tokens_key("a@golia.jp"), "push:tokens:a@golia.jp");
    }
}
