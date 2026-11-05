//! `move_category` — switch one reader's copy of a thread from one
//! category to another.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Move `user`'s copy of `thread_id` into `new_category`, for them
    /// alone (`standing.rs`). No-op if it is already there. Returns true if
    /// they hold the conversation.
    pub fn move_category(
        &self,
        user: &str,
        thread_id: &str,
        new_category: &str,
    ) -> io::Result<bool> {
        if !self
            .store()
            .hexists(keys::thread_user(user, thread_id).as_bytes(), b"tid")?
        {
            return Ok(false);
        }
        self.file_for_reader(user, thread_id, new_category)?;
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
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        // The aggregate index that derives the counters, too — without
        // it every count reads zero, which looks exactly like a broken
        // count rather than a store that was never fully booted.
        s.ensure_admin_indexes();
        s
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
            is_own: false,
        }
    }

    #[test]
    fn moves_between_categories() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, "inbox")).unwrap();

        let count = |cat: &str| {
            s.count_thread_ids_by_category_via_table(u, cat, crate::ArchiveScope::Live)
                .unwrap()
        };
        assert_eq!(count("inbox"), 1);
        assert_eq!(count("social"), 0);

        assert!(s.move_category(u, "t1", "social").unwrap());

        assert_eq!(count("inbox"), 0, "the old category must let it go");
        assert_eq!(count("social"), 1, "the new one must hold it");
        let row = s.get_thread_for_user(u, "t1").unwrap().unwrap();
        assert_eq!(row.category, "social");
    }

    #[test]
    fn same_category_is_noop() {
        let s = store();
        let u = "u@x.com";
        s.record_message_arrival(&arr("t1", u, "inbox")).unwrap();
        assert!(s.move_category(u, "t1", "inbox").unwrap());
        // Still exactly one thread on the inbox category axis.
        assert_eq!(
            s.count_thread_ids_by_category_via_table(u, "inbox", crate::ArchiveScope::Live)
                .unwrap(),
            1
        );
    }

    #[test]
    fn missing_thread_returns_false() {
        let s = store();
        assert!(!s.move_category("u@x.com", "nope", "social").unwrap());
    }

    #[test]
    fn list_filter_picks_up_new_category() {
        let s = store();
        // the category axis is served from the declared table
        s.ensure_thread_table();
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
