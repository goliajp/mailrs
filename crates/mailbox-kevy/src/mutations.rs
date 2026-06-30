//! Thread-level mutations — archive / unarchive / pin / unpin / delete.
//!
//! Phase 7.9. Same two-step pattern as `mark_seen` (hset + zrem outside
//! the atomic block) until kevy 1.16 lands `AtomicCtx::{zrem, hdel}`.
//! `delete_thread` is the heaviest one: drops the row from 6 indexes
//! and the hash itself.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Flip `archived` on or off for `thread_id`. Toggles both the
    /// `archived` field on the row and membership in the per-user
    /// archived zset (added when archived=true, removed otherwise).
    ///
    /// Returns true if the row existed.
    pub fn set_archived(&self, user: &str, thread_id: &str, archived: bool) -> io::Result<bool> {
        self.toggle_flag(
            user,
            thread_id,
            "archived",
            archived,
            keys::user_threads_archived,
        )
    }

    /// Flip `pinned` on or off. Same shape as `set_archived`.
    pub fn set_pinned(&self, user: &str, thread_id: &str, pinned: bool) -> io::Result<bool> {
        self.toggle_flag(user, thread_id, "pinned", pinned, keys::user_threads_pinned)
    }

    /// Flip `has_action` on or off. Same shape.
    pub fn set_has_action(
        &self,
        user: &str,
        thread_id: &str,
        has_action: bool,
    ) -> io::Result<bool> {
        self.toggle_flag(
            user,
            thread_id,
            "has_action",
            has_action,
            keys::user_threads_has_action,
        )
    }

    /// Flip `starred` on or off. Same shape — toggles `starred` field
    /// + per-user `starred` zset membership.
    pub fn set_starred(&self, user: &str, thread_id: &str, starred: bool) -> io::Result<bool> {
        self.toggle_flag(
            user,
            thread_id,
            "starred",
            starred,
            keys::user_threads_starred,
        )
    }

    /// Common path: read latest_date (for the zadd score), hset the
    /// boolean field, and add or remove from the matching index zset.
    fn toggle_flag<F>(
        &self,
        user: &str,
        thread_id: &str,
        field: &'static str,
        on: bool,
        index_key_fn: F,
    ) -> io::Result<bool>
    where
        F: FnOnce(&str) -> String,
    {
        let thread_key = keys::thread(thread_id);
        if !self.store().hexists(thread_key.as_bytes(), b"count")? {
            return Ok(false);
        }
        let val: &[u8] = if on { b"1" } else { b"0" };
        self.store()
            .hset(thread_key.as_bytes(), &[(field.as_bytes(), val)])?;
        let idx = index_key_fn(user);
        if on {
            // Need a score — use the row's latest_date so the index
            // stays sortable by recency.
            let score = self
                .store()
                .hget(thread_key.as_bytes(), b"latest_date")?
                .and_then(|v| {
                    std::str::from_utf8(&v)
                        .ok()
                        .and_then(|s| s.parse::<i64>().ok())
                })
                .unwrap_or(0);
            self.store()
                .zadd(idx.as_bytes(), &[(score as f64, thread_id.as_bytes())])?;
        } else {
            self.store().zrem(idx.as_bytes(), &[thread_id.as_bytes()])?;
        }
        Ok(true)
    }

    /// Hard-delete `thread_id` for `user`. Removes the row hash + drops
    /// it from every index zset the row could be in. Idempotent: a
    /// re-call after deletion is a no-op returning false.
    ///
    /// Reads `category` BEFORE the deletion so we know which
    /// per-category zset to clean — that index is keyed by the
    /// category string, not derivable from the tid alone.
    pub fn delete_thread(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        let category = self
            .store()
            .hget(thread_key.as_bytes(), b"category")?
            .and_then(|v| String::from_utf8(v).ok());
        if category.is_none() {
            // hash doesn't exist
            return Ok(false);
        }
        let cat = category.unwrap();
        // hdel each field — kevy 1.15 has hdel but no "hclear" so we
        // explicitly list the fields ThreadRow knows about.
        let fields: &[&[u8]] = &[
            b"subject",
            b"senders_csv",
            b"count",
            b"unread_count",
            b"latest_date",
            b"latest_preview",
            b"category",
            b"importance_level",
            b"importance_score",
            b"requires_action",
            b"pinned",
            b"archived",
            b"has_action",
            b"sent_count",
            b"starred",
        ];
        self.store().hdel(thread_key.as_bytes(), fields)?;
        // drop from every index zset the row could appear in.
        let indexes = [
            keys::user_threads_by_activity(user),
            keys::user_threads_by_category(user, &cat),
            keys::user_threads_pinned(user),
            keys::user_threads_archived(user),
            keys::user_threads_has_unread(user),
            keys::user_threads_has_action(user),
            keys::user_threads_starred(user),
        ];
        for idx in &indexes {
            self.store().zrem(idx.as_bytes(), &[thread_id.as_bytes()])?;
        }
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

    fn arr<'a>(tid: &'a str, user: &'a str) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread: true,
        }
    }

    #[test]
    fn set_archived_round_trip() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();

        assert!(s.set_archived(u, "t1", true).unwrap());
        assert!(s.get_thread("t1").unwrap().unwrap().archived);
        let arch = keys::user_threads_archived(u);
        assert_eq!(s.store().zcard(arch.as_bytes()).unwrap(), 1);

        assert!(s.set_archived(u, "t1", false).unwrap());
        assert!(!s.get_thread("t1").unwrap().unwrap().archived);
        assert_eq!(s.store().zcard(arch.as_bytes()).unwrap(), 0);
    }

    #[test]
    fn set_pinned_uses_pinned_zset() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        assert!(s.set_pinned(u, "t1", true).unwrap());
        let pinned = keys::user_threads_pinned(u);
        assert_eq!(s.store().zcard(pinned.as_bytes()).unwrap(), 1);
        // appears in the pinned filter list
        let f = crate::ListThreadsFilter {
            pinned: true,
            ..Default::default()
        };
        let (rows, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].thread_id, "t1");
    }

    #[test]
    fn delete_thread_clears_all_indexes() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u)).unwrap();
        s.set_archived(u, "t1", true).unwrap();
        s.set_pinned(u, "t1", true).unwrap();
        s.set_has_action(u, "t1", true).unwrap();

        // every zset has the row now
        for idx in [
            keys::user_threads_by_activity(u),
            keys::user_threads_by_category(u, "inbox"),
            keys::user_threads_pinned(u),
            keys::user_threads_archived(u),
            keys::user_threads_has_unread(u),
            keys::user_threads_has_action(u),
        ] {
            assert_eq!(s.store().zcard(idx.as_bytes()).unwrap(), 1, "idx {idx}");
        }

        assert!(s.delete_thread(u, "t1").unwrap());
        assert!(s.get_thread("t1").unwrap().is_none());
        for idx in [
            keys::user_threads_by_activity(u),
            keys::user_threads_by_category(u, "inbox"),
            keys::user_threads_pinned(u),
            keys::user_threads_archived(u),
            keys::user_threads_has_unread(u),
            keys::user_threads_has_action(u),
        ] {
            assert_eq!(s.store().zcard(idx.as_bytes()).unwrap(), 0, "idx {idx}");
        }
    }

    #[test]
    fn delete_missing_returns_false() {
        let s = store();
        assert!(!s.delete_thread("u@x.com", "nope").unwrap());
    }

    #[test]
    fn flag_toggle_on_missing_thread_returns_false() {
        let s = store();
        assert!(!s.set_archived("u@x.com", "nope", true).unwrap());
        assert!(!s.set_pinned("u@x.com", "nope", true).unwrap());
    }
}
