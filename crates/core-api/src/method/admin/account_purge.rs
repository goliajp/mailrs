//! Deleting an account is two steps. The delete parks the maildir, drops
//! the account record and marks the address pending purge; the purge,
//! once the retention period has passed, removes everything the address
//! still owns: the parked maildir, its rows in the mail store, its
//! network-kevy keys and its group memberships.

use serde::{Deserialize, Serialize};

/// `POST ?dry_run=false` purges; without `dry_run`, or with `true`, it only
/// counts. `address` narrows the pass to one pending address.
pub const PATH_ACCOUNT_PURGE: &str = "/v1/admin/maintenance:account-purge";

/// How many entries of one kind a purge found (dry run) or removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeCount {
    /// The store's own name for the kind: a key prefix, a table, a group.
    pub name: String,
    pub count: u64,
}

/// What a purge found or removed for one address, by store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPurgeRow {
    pub address: String,
    /// When the account was deleted (unix seconds).
    pub deleted_at: i64,
    /// Files in the parked maildirs.
    pub maildir_files: u64,
    pub maildir_bytes: u64,
    /// Rows of the core's mail store, by table or key family. The two
    /// cores name these differently: fastcore by kevy family, the SQL
    /// core by table.
    pub mail_store: Vec<PurgeCount>,
    /// Network-kevy keys by registered prefix, and the group memberships
    /// the address held.
    pub network: Vec<PurgeCount>,
    /// Network-kevy keys that still name the address after the pass (on a
    /// dry run: that no registered family accounts for), by the part of the
    /// key before the address. Not removed: a family missing from the
    /// registry, to be added there.
    pub unregistered: Vec<PurgeCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPurgeResponse {
    pub dry_run: bool,
    /// The retention period in force, in days.
    pub retention_days: u64,
    /// Addresses pending purge, due or not: what tells "nothing due" from
    /// "nothing read".
    pub pending: usize,
    /// Pending addresses whose retention period has passed.
    pub due: usize,
    /// One row per due address, purged unless `dry_run`.
    pub rows: Vec<AccountPurgeRow>,
    /// Due addresses that are an account again and were left alone.
    pub skipped_live: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_a_maintenance_route() {
        assert_eq!(PATH_ACCOUNT_PURGE, "/v1/admin/maintenance:account-purge");
    }

    #[test]
    fn the_response_roundtrips() {
        let r = AccountPurgeResponse {
            dry_run: true,
            retention_days: 30,
            pending: 1,
            due: 1,
            rows: vec![AccountPurgeRow {
                address: "a@x.test".into(),
                deleted_at: 1,
                maildir_files: 2,
                maildir_bytes: 3,
                mail_store: vec![PurgeCount {
                    name: "thread_user".into(),
                    count: 4,
                }],
                network: vec![],
                unregistered: vec![],
            }],
            skipped_live: vec![],
        };
        let back: AccountPurgeResponse =
            serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
