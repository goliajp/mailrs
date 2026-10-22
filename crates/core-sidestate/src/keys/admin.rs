//! Admin state that the webapi writes and other processes read.

/// The audit log, newest first (LPUSH).
pub const AUDIT_LOG: &[u8] = b"admin:audit_log";

/// Next audit entry id.
pub const AUDIT_LOG_COUNTER: &[u8] = b"admin:audit_log:counter";

/// Runtime settings the admin page edits; the receiver reads its limits
/// from here.
pub const SYSTEM_CONFIG: &[u8] = b"admin:system-config";

/// Who changed each [`SYSTEM_CONFIG`] field, and when.
pub const SYSTEM_CONFIG_META: &[u8] = b"admin:system-config:meta";

/// Admin-managed greylist allow / deny entries; the receiver syncs them.
pub const GREYLIST_LOCAL_LISTS: &[u8] = b"admin:greylist:local-lists";

/// Next [`GREYLIST_LOCAL_LISTS`] entry id.
pub const GREYLIST_LOCAL_LISTS_COUNTER: &str = "admin:greylist:local-lists:counter";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_keys_keep_their_prod_spelling() {
        assert_eq!(AUDIT_LOG, b"admin:audit_log");
        assert_eq!(AUDIT_LOG_COUNTER, b"admin:audit_log:counter");
        assert_eq!(SYSTEM_CONFIG, b"admin:system-config");
        assert_eq!(SYSTEM_CONFIG_META, b"admin:system-config:meta");
        assert_eq!(GREYLIST_LOCAL_LISTS, b"admin:greylist:local-lists");
        assert_eq!(
            GREYLIST_LOCAL_LISTS_COUNTER,
            "admin:greylist:local-lists:counter"
        );
    }
}
