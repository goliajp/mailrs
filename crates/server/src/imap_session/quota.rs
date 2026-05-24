//! IMAP GETQUOTA / GETQUOTAROOT (RFC 2087) handlers.
//!
//! mailrs uses one quota root per user (the username string).
//! Storage is measured in bytes via
//! `PgMailboxStore::user_storage_usage`; the wire format converts
//! to KB (`quota / 1024`). Limit comes from the per-account
//! row in `domain_store` or defaults to 1 GiB.

use mailrs_imap_proto::{format_no, format_ok, format_quota, format_quotaroot};

use super::ImapSession;

impl ImapSession {
    pub(super) async fn handle_getquota(&self, tag: &str, quotaroot: &str) -> Vec<String> {
        let username = match self.authenticated_username(tag) {
            Ok(u) => u,
            Err(resp) => return resp,
        };

        // quotaroot is the username (user-level quota)
        if quotaroot != username {
            return vec![format_no(tag, "permission denied")];
        }

        let usage = self.mailbox_store.user_storage_usage(username).await;
        let quota = if let Some(ref ds) = self.domain_store {
            ds.get_quota(username)
                .await
                .ok()
                .flatten()
                .unwrap_or(1_073_741_824)
        } else {
            1_073_741_824 // default 1GB
        };

        // IMAP QUOTA uses KB
        let usage_kb = usage / 1024;
        let limit_kb = quota as u64 / 1024;

        vec![
            format_quota(quotaroot, usage_kb, limit_kb),
            format_ok(tag, "GETQUOTA completed"),
        ]
    }

    pub(super) async fn handle_getquotaroot(&self, tag: &str, mailbox: &str) -> Vec<String> {
        let username = match self.authenticated_username(tag) {
            Ok(u) => u,
            Err(resp) => return resp,
        };

        let usage = self.mailbox_store.user_storage_usage(username).await;
        let quota = if let Some(ref ds) = self.domain_store {
            ds.get_quota(username)
                .await
                .ok()
                .flatten()
                .unwrap_or(1_073_741_824)
        } else {
            1_073_741_824
        };

        let usage_kb = usage / 1024;
        let limit_kb = quota as u64 / 1024;

        vec![
            format_quotaroot(mailbox, username),
            format_quota(username, usage_kb, limit_kb),
            format_ok(tag, "GETQUOTAROOT completed"),
        ]
    }
}
