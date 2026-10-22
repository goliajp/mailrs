//! Mobile push tokens: the webapi registers them, intake reads them to
//! notify on new mail.

/// One user's tokens: hash, field = the token, value = metadata.
pub fn tokens_key(user: &str) -> String {
    format!("push:tokens:{user}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_keys_keep_their_prod_spelling() {
        assert_eq!(tokens_key("a@golia.jp"), "push:tokens:a@golia.jp");
    }
}
