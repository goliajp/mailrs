//! "Assigned to me": a reader's team conversations one member answers,
//! off the `by_user_assignee` ORDERPATH.

use std::io;

use super::ArchiveScope;
use crate::KevyMailboxStore;

impl KevyMailboxStore {
    /// `user`'s live conversations assigned to `assignee`, newest first;
    /// with `max_activity`, only those older than it.
    pub fn list_thread_ids_assigned_via_table(
        &self,
        user: &str,
        assignee: &str,
        max_activity: Option<i64>,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        let Some(max) = max_activity else {
            return self.query_orderpath(
                b"threaduser.by_user_assignee",
                user,
                b"assignee",
                assignee,
                ArchiveScope::Live,
                limit,
            );
        };
        let mut eqs = vec![
            (b"user".to_vec(), user.as_bytes().to_vec()),
            (b"assignee".to_vec(), assignee.as_bytes().to_vec()),
        ];
        ArchiveScope::Live.push_to(&mut eqs);
        let clause = kevy_index::WhereClause {
            eqs,
            range: Some((
                b"activity".to_vec(),
                i64::MIN.to_string().into_bytes(),
                max.to_string().into_bytes(),
            )),
        };
        self.run_orderpath(b"threaduser.by_user_assignee", user, &clause, limit)
    }
}
