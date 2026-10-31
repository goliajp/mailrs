//! The Trash: a conversation a reader deleted is kept, out of every other
//! list, until they restore it, delete it for good, or 30 days pass.
//!
//! Two columns on the reader's membership row: `trashed` (an equality
//! component of every ORDERPATH prefix and the Trash list's own flag
//! index) and `trashed_at` (keyed like `snoozed_until`, so the purge's
//! "what is due" is an empty range scan when nothing is).

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// Move `thread_id` to `user`'s Trash at `now`, or out of it. `true`
    /// when the row existed. Nothing else on the row changes, so
    /// restoring puts the conversation back in the list it left.
    pub fn set_trashed(
        &self,
        user: &str,
        thread_id: &str,
        trashed: bool,
        now: i64,
    ) -> io::Result<bool> {
        let key = keys::thread_user(user, thread_id);
        let at = match trashed {
            true => now.to_string(),
            false => "0".to_string(),
        };
        self.store()
            .atomic(|ctx| {
                if !ctx.hexists(key.as_bytes(), b"tid")? {
                    return Ok(false);
                }
                ctx.hset(
                    key.as_bytes(),
                    &[
                        (b"trashed" as &[u8], crate::thread_row::flag(trashed)),
                        (b"trashed_at", at.as_bytes()),
                    ],
                )?;
                Ok(true)
            })
            .map_err(io::Error::from)
    }

    /// Up to `limit` `(user, thread id)` that went to the Trash at or
    /// before `cutoff`. An empty range scan when nothing is due.
    pub fn trashed_before(&self, cutoff: i64, limit: usize) -> io::Result<Vec<(String, String)>> {
        let (rows, _) = self.store().idx_query(
            b"threaduser.trashed_at",
            &kevy_embedded::IndexValue::I64(1),
            &kevy_embedded::IndexValue::I64(cutoff),
            None,
            limit,
        )?;
        let prefix = "mailrs:threaduser:";
        Ok(rows
            .into_iter()
            .filter_map(|(key, _)| {
                let key = String::from_utf8(key).ok()?;
                // an address has no `:`, so the first one ends it
                let (user, tid) = key.strip_prefix(prefix)?.split_once(':')?;
                Some((user.to_string(), tid.to_string()))
            })
            .collect())
    }

    /// Every conversation in `user`'s Trash, for emptying it.
    pub fn trashed_threads(&self, user: &str) -> io::Result<Vec<String>> {
        let total = self.count_thread_ids_by_flag_filtered(user, "trashed", &[])?;
        self.list_thread_ids_by_flag_filtered(user, "trashed", &[], total, 0, None)
    }
}

#[cfg(test)]
mod tests;
