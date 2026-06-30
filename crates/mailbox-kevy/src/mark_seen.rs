//! `mark_seen` — flip a thread from unread → seen.
//!
//! Phase 7.8. Two ops (hset + zrem) because `AtomicCtx` in 1.15.0
//! exposes `hset / hincrby / zadd / zincrby / zscore` but NOT `zrem`
//! or `hdel`. Without those there's no way to remove the row from
//! the `has_unread` index inside the atomic block.
//!
//! We do this:
//!   1. `hset thread:<tid> unread_count = 0`
//!   2. `zrem user:<u>:threads:has_unread <tid>`
//!
//! Step 2 outside the atomic block is acceptable here: the worst case
//! is a millisecond window where the row reads as `unread_count = 0`
//! but the index still says "unread". The conversation list still
//! returns the row (just with the correct counter); a refresh
//! reconciles. See `.claude/notes/kevy-feedback-atomicctx-zrem-hdel-2026-07-01.md`
//! for the feedback note that asks the kevy team to add zrem/hdel
//! to AtomicCtx so this method can collapse to a single atomic block.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Mark `thread_id` as seen for `user` — zero the unread counter
    /// and drop the row from the `has_unread` index.
    ///
    /// Idempotent: re-applying produces the same state. Returns `true`
    /// if the thread row was found (regardless of whether the unread
    /// count actually flipped); `false` if the row doesn't exist.
    pub fn mark_seen(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        let exists = self
            .store()
            .hexists(thread_key.as_bytes(), b"unread_count")?;
        if !exists {
            return Ok(false);
        }
        self.store()
            .hset(thread_key.as_bytes(), &[(b"unread_count" as &[u8], b"0")])?;
        let idx = keys::user_threads_has_unread(user);
        self.store().zrem(idx.as_bytes(), &[thread_id.as_bytes()])?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageArrival;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));
        KevyMailboxStore::new(s)
    }

    fn arr<'a>(tid: &'a str, user: &'a str, unread: bool) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread,
        }
    }

    #[test]
    fn mark_seen_zeros_unread_and_drops_from_index() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, true)).unwrap();
        let unread_idx = keys::user_threads_has_unread(u);
        assert_eq!(s.store().zcard(unread_idx.as_bytes()).unwrap(), 1);

        let flipped = s.mark_seen(u, "t1").unwrap();
        assert!(flipped);

        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.unread_count, 0);
        assert_eq!(s.store().zcard(unread_idx.as_bytes()).unwrap(), 0);
    }

    #[test]
    fn mark_seen_missing_thread_returns_false() {
        let s = store();
        assert!(!s.mark_seen("u@x.com", "nope").unwrap());
    }

    #[test]
    fn mark_seen_is_idempotent() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, true)).unwrap();
        assert!(s.mark_seen(u, "t1").unwrap());
        assert!(s.mark_seen(u, "t1").unwrap()); // 2nd call OK
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.unread_count, 0);
    }

    #[test]
    fn list_after_mark_seen_excludes_from_has_unread_filter() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("a", u, true)).unwrap();
        s.record_message_arrival(&arr("b", u, true)).unwrap();
        s.mark_seen(u, "a").unwrap();

        let filter = crate::ListThreadsFilter {
            has_unread: true,
            ..Default::default()
        };
        let (rows, total) = s.list_threads_by_activity(u, &filter, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].thread_id, "b");
    }
}
