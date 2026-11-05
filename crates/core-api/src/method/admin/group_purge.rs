//! Deleting an email group is two steps, as deleting an account is. The
//! delete takes the group out of use at once (its row and member set go)
//! and marks it pending; the purge, once the retention period of deleted
//! accounts has passed, removes its team working state (the
//! `shared:{gid}:*` keys and the reply claims members hold for it) and
//! the reply claims of its conversations in the core's own store. The mail
//! itself stays in the members' mailboxes.

use serde::{Deserialize, Serialize};

use super::PurgeCount;

/// `POST ?dry_run=false` purges; without `dry_run`, or with `true`, it only
/// counts. `gid` narrows the pass to one pending group.
pub const PATH_GROUP_PURGE: &str = "/v1/admin/maintenance:group-purge";

/// What a purge found or removed for one deleted group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupPurgeRow {
    pub gid: i64,
    /// The group's address when it was deleted.
    pub address: String,
    /// When the group was deleted (unix seconds).
    pub deleted_at: i64,
    /// Network-kevy keys by family (`shared:{gid}:{conv}:notes`, ...),
    /// and the claims members held for the group.
    pub network: Vec<PurgeCount>,
    /// The reply claims of the group's conversations in the core's own
    /// store. The two cores name these differently: fastcore by kevy
    /// family, the SQL core by table.
    pub mail_store: Vec<PurgeCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupPurgeResponse {
    pub dry_run: bool,
    /// The retention period in force, in days.
    pub retention_days: u64,
    /// Groups pending purge, due or not.
    pub pending: usize,
    /// Pending groups whose retention period has passed.
    pub due: usize,
    /// One row per due group, purged unless `dry_run`.
    pub rows: Vec<GroupPurgeRow>,
    /// Due groups whose row is there again and that were left alone.
    pub skipped_live: Vec<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_response_roundtrips() {
        let r = GroupPurgeResponse {
            dry_run: true,
            retention_days: 30,
            pending: 1,
            due: 1,
            rows: vec![GroupPurgeRow {
                gid: 3,
                address: "team@x.test".into(),
                deleted_at: 1,
                network: vec![PurgeCount {
                    name: "msgids".into(),
                    count: 1,
                }],
                mail_store: vec![],
            }],
            skipped_live: vec![],
        };
        let back: GroupPurgeResponse =
            serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
