//! Deleting an account is two steps, and a third undoes the first. The delete parks the maildir, drops
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

/// `POST` with a [`RestoreAccountRequest`]: a deleted account whose purge
/// has not run, back as it was. 204; 409 with a [`RestoreRefused`] when
/// there is nothing to restore or the parked maildir to restore is not
/// clear.
pub const PATH_RESTORE_ACCOUNT: &str = "/v1/admin/accounts/{address}/restore";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreAccountRequest {
    /// Who restores it, for the audit row.
    pub by: String,
    /// Which parked maildir to move back, by the delete time its directory
    /// is named with; needed only when the address was deleted more than
    /// once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked: Option<String>,
}

/// What a restore did with the mail its delete withheld, by message: a
/// send whose time is still ahead waits for it again; one whose time
/// passed while the account was deleted is not sent, and is back as a
/// draft of the account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreAccountResponse {
    pub rescheduled: u64,
    pub returned_as_drafts: u64,
}

/// Why a restore was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreRefusal {
    /// The address is an account.
    Live,
    /// The address is not waiting for a purge: never deleted, or purged.
    NotPending,
    /// Deleted before restoring was possible: the record is not kept.
    NoRecord,
    /// Several parked maildirs, and none was named.
    SeveralParked,
    /// The named parked maildir is not there.
    NoSuchParked,
    /// Something is already where the maildir would go back.
    MaildirInTheWay,
}

/// The 409 body of a refused restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreRefused {
    pub refused: RestoreRefusal,
    /// The parked maildirs there are, by their delete time.
    pub parked: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_a_maintenance_route() {
        assert_eq!(PATH_ACCOUNT_PURGE, "/v1/admin/maintenance:account-purge");
    }

    #[test]
    fn a_refusal_reads_in_snake_case() {
        let r = RestoreRefused {
            refused: RestoreRefusal::SeveralParked,
            parked: vec!["1".into(), "2".into()],
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"refused":"several_parked","parked":["1","2"]}"#);
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
