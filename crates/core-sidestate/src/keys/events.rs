//! Live-update frames: intake SETs one per change, the webapi's feed
//! consumers pick up every key under the prefix.

/// What every frame key starts with.
pub const NOTIFY_PREFIX: &[u8] = b"mailrs:events:notify:";

/// One frame, named by the nanosecond it was written.
pub fn notify_key(nanos: u128) -> String {
    format!("mailrs:events:notify:{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_keys_keep_their_prod_spelling() {
        assert_eq!(NOTIFY_PREFIX, b"mailrs:events:notify:");
        assert_eq!(notify_key(17), "mailrs:events:notify:17");
        assert!(notify_key(17).as_bytes().starts_with(NOTIFY_PREFIX));
    }
}
