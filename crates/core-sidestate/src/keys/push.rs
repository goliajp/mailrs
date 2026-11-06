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

/// When each of one user's devices, on any channel, last took a push:
/// hash, field = `{channel}:{device id}`, value = epoch seconds.
pub fn used_key(user: &str) -> String {
    format!("push:used:{}", user.to_lowercase())
}

/// Devices on any channel a person removed from another device, so they do
/// not register themselves back: hash, field = `{channel}:{device id}`,
/// value = epoch seconds; each field expires on its own.
pub fn removed_key(user: &str) -> String {
    format!("push:removed:{}", user.to_lowercase())
}

/// The session that last registered each of one user's devices, so ending
/// a session ends its pushes: hash, field = `{channel}:{device id}`, value
/// = the session's ref.
pub fn session_link_key(user: &str) -> String {
    format!("push:session:{}", user.to_lowercase())
}

/// One user's open UnifiedPush window: a counter that lives as long as the
/// window.
pub fn unified_push_window_key(user: &str) -> String {
    format!("push:upwindow:{}", user.to_lowercase())
}

/// Present while a ping at the end of one user's UnifiedPush window is due.
pub fn unified_push_trailing_key(user: &str) -> String {
    format!("push:uptrailing:{}", user.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_keys_keep_their_prod_spelling() {
        assert_eq!(tokens_key("a@golia.jp"), "push:tokens:a@golia.jp");
    }
}
