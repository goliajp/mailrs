//! Admin state that the webapi writes and other processes read.

/// The audit log, newest first (LPUSH).
pub const AUDIT_LOG: &[u8] = b"admin:audit_log";

/// Next audit entry id.
pub const AUDIT_LOG_COUNTER: &[u8] = b"admin:audit_log:counter";

/// The audit chain: hash field per sealed row id (`families::audit_chain`).
pub const AUDIT_CHAIN: &[u8] = b"admin:audit_log:chain";

/// The last sealed row: its id and hash.
pub const AUDIT_CHAIN_HEAD: &[u8] = b"admin:audit_log:chain:head";

/// Where the chain starts: the oldest sealed row kept, and the hash before it.
pub const AUDIT_CHAIN_ANCHOR: &[u8] = b"admin:audit_log:chain:anchor";

/// The last daily verification, JSON; what `/api/health` reports.
pub const AUDIT_CHAIN_STATUS: &[u8] = b"admin:audit_log:chain:status";

/// The last row id written to the replica directory.
pub const AUDIT_CHAIN_REPLICATED: &[u8] = b"admin:audit_log:chain:replicated";

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
        assert_eq!(AUDIT_CHAIN, b"admin:audit_log:chain");
        assert_eq!(AUDIT_CHAIN_HEAD, b"admin:audit_log:chain:head");
        assert_eq!(AUDIT_CHAIN_ANCHOR, b"admin:audit_log:chain:anchor");
        assert_eq!(AUDIT_CHAIN_STATUS, b"admin:audit_log:chain:status");
        assert_eq!(AUDIT_CHAIN_REPLICATED, b"admin:audit_log:chain:replicated");
        assert_eq!(SYSTEM_CONFIG, b"admin:system-config");
        assert_eq!(SYSTEM_CONFIG_META, b"admin:system-config:meta");
        assert_eq!(GREYLIST_LOCAL_LISTS, b"admin:greylist:local-lists");
        assert_eq!(
            GREYLIST_LOCAL_LISTS_COUNTER,
            "admin:greylist:local-lists:counter"
        );
    }
}
