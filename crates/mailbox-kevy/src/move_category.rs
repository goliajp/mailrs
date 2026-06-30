//! `move_category` — switch a thread from one category to another.
//!
//! Phase 7.10. Three ops because `AtomicCtx` lacks `zrem`:
//!   1. read old category from the row
//!   2. hset thread:<tid> category = new
//!   3. zrem user:<u>:threads:by_category:<old>
//!   4. zadd user:<u>:threads:by_category:<new>
//!
//! Brief inconsistent window: between step 3 and step 4 the thread is
//! in neither category index. A reader hitting
//! `list_threads_by_activity(category=<old>)` sees a clean exit; a
//! reader hitting `category=<new>` sees a missing row. Both reconcile
//! on the next call. When kevy 1.16 lands `AtomicCtx::{zrem, hdel}`
//! all 4 ops collapse into one atomic block.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Move `thread_id` from its current category to `new_category`.
    /// No-op if the row is already in `new_category`. Returns true if
    /// the row was found.
    pub fn move_category(
        &self,
        user: &str,
        thread_id: &str,
        new_category: &str,
    ) -> io::Result<bool> {
        let thread_key = keys::thread(thread_id);
        let old = self
            .store()
            .hget(thread_key.as_bytes(), b"category")?
            .and_then(|v| String::from_utf8(v).ok());
        let Some(old) = old else {
            return Ok(false);
        };
        if old == new_category {
            return Ok(true);
        }
        let score = self
            .store()
            .hget(thread_key.as_bytes(), b"latest_date")?
            .and_then(|v| {
                std::str::from_utf8(&v)
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok())
            })
            .unwrap_or(0);

        // Overwrite category field, then swap zset membership.
        self.store().hset(
            thread_key.as_bytes(),
            &[(b"category" as &[u8], new_category.as_bytes())],
        )?;
        let old_idx = keys::user_threads_by_category(user, &old);
        self.store()
            .zrem(old_idx.as_bytes(), &[thread_id.as_bytes()])?;
        let new_idx = keys::user_threads_by_category(user, new_category);
        self.store()
            .zadd(new_idx.as_bytes(), &[(score as f64, thread_id.as_bytes())])?;
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

    fn arr<'a>(tid: &'a str, user: &'a str, category: &'a str) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: 100,
            latest_preview: "preview",
            category,
            unread: true,
        }
    }

    #[test]
    fn moves_between_categories() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, "inbox")).unwrap();

        let inbox = keys::user_threads_by_category(u, "inbox");
        let social = keys::user_threads_by_category(u, "social");
        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 1);
        assert_eq!(s.store().zcard(social.as_bytes()).unwrap(), 0);

        assert!(s.move_category(u, "t1", "social").unwrap());

        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 0);
        assert_eq!(s.store().zcard(social.as_bytes()).unwrap(), 1);
        let row = s.get_thread("t1").unwrap().unwrap();
        assert_eq!(row.category, "social");
    }

    #[test]
    fn same_category_is_noop() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, "inbox")).unwrap();
        assert!(s.move_category(u, "t1", "inbox").unwrap());
        // Still 1 row in inbox, nothing in any other category zset.
        let inbox = keys::user_threads_by_category(u, "inbox");
        assert_eq!(s.store().zcard(inbox.as_bytes()).unwrap(), 1);
    }

    #[test]
    fn missing_thread_returns_false() {
        let s = store();
        assert!(!s.move_category("u@x.com", "nope", "social").unwrap());
    }

    #[test]
    fn list_filter_picks_up_new_category() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, "inbox")).unwrap();
        s.move_category(u, "t1", "promotions").unwrap();

        let f = crate::ListThreadsFilter {
            category: Some("promotions"),
            ..Default::default()
        };
        let (rows, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].thread_id, "t1");
    }
}
