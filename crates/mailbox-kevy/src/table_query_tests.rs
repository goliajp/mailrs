//! Tests for `table_query`, in their own file only because the module
//! they belong to sits at the size limit — `file-size.md` counts a single
//! trailing `mod tests` as free, and these are two named modules.

#![cfg(test)]

use crate::{KevyMailboxStore, keys};

mod orderpath_read_tests {
    use super::*;
    use crate::thread_row;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn row(tid: &str, activity: i64, category: &str) -> thread_row::ThreadRow {
        thread_row::ThreadRow {
            thread_id: tid.into(),
            subject: String::new(),
            senders_csv: String::new(),
            count: 1,
            unread_count: 0,
            latest_date: activity,
            latest_preview: String::new(),
            category: category.into(),
            importance_level: "normal".into(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred: false,
            snoozed_until: 0,
        }
    }

    /// The engine's answer must be the order the UI asks for: newest
    /// first, scoped to one user and one bucket, with rows belonging to
    /// other users or other buckets absent.
    #[test]
    fn orderpath_returns_newest_first_scoped_to_the_user() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        for (tid, when) in [("old", 100), ("newest", 300), ("middle", 200)] {
            st.write_thread_user_if_changed("alice@x.com", &row(tid, when, "inbox"))
                .unwrap();
        }
        // A different user's copy of a thread, and a junk thread — both
        // must stay out of alice's inbox answer.
        st.write_thread_user_if_changed("bob@x.com", &row("bobs", 999, "inbox"))
            .unwrap();
        st.write_thread_user_if_changed("alice@x.com", &row("spammy", 999, "spam"))
            .unwrap();

        let got = st
            .list_thread_ids_by_bucket_via_table(
                "alice@x.com",
                "inbox",
                crate::ArchiveScope::Live,
                50,
            )
            .unwrap();
        assert_eq!(got, vec!["newest", "middle", "old"]);
    }

    /// Threads whose ids exceed kevy's 255-byte string component cap
    /// must still be indexed — that is the whole reason the sort ends
    /// on a folded hash rather than on the id itself.
    #[test]
    fn an_overlong_thread_id_is_still_indexed() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        let long_tid = format!("<{}@example.com>", "x".repeat(300));
        st.write_thread_user_if_changed("alice@x.com", &row(&long_tid, 500, "inbox"))
            .unwrap();
        st.write_thread_user_if_changed("alice@x.com", &row("short", 100, "inbox"))
            .unwrap();

        let got = st
            .list_thread_ids_by_bucket_via_table(
                "alice@x.com",
                "inbox",
                crate::ArchiveScope::Live,
                50,
            )
            .unwrap();
        assert_eq!(got, vec![long_tid, "short".to_string()]);
    }
}

mod flag_axis_tests {
    use super::*;
    use crate::thread_row;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    /// Seed a starred thread the way the product does: write the
    /// aggregate, then flip the flag through the mutator that owns it.
    ///
    /// `starred` on a `ThreadRow` no longer reaches the membership row
    /// on its own — it is one user's state and `thread_user_pairs`
    /// stopped deriving it from the shared hash, which is the fix these
    /// tests sit downstream of.
    fn seed_starred(st: &KevyMailboxStore, user: &str, tid: &str, activity: i64) {
        st.upsert_thread(user, &flagged(tid, activity, true))
            .unwrap();
        st.set_starred(user, tid, true).unwrap();
    }

    fn flagged(tid: &str, activity: i64, starred: bool) -> thread_row::ThreadRow {
        thread_row::ThreadRow {
            thread_id: tid.into(),
            subject: String::new(),
            senders_csv: String::new(),
            count: 1,
            unread_count: 0,
            latest_date: activity,
            latest_preview: String::new(),
            category: "inbox".into(),
            importance_level: "normal".into(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred,
            snoozed_until: 0,
        }
    }

    /// Does `SORT` order the whole match set, or only the rows a page
    /// happened to select?
    ///
    /// This decides whether the flag axes can be served from a single
    /// index per flag (keyed on the flag, `FILTER user`, `SORT
    /// activity`) or whether each needs its own composite ORDERPATH.
    /// Does a field the `TableSpec` never declared break the row?
    ///
    /// This decides the cost of moving per-user state onto this row
    /// (RFC 20260730). If undeclared fields are simply carried, the
    /// counters and display fields can land here as payload and the
    /// spec never changes — no `table_drop`, no rebuilding 30,510 rows'
    /// indexes at boot. If they are rejected or silently drop the row
    /// out of its indexes, the migration needs a rehearsal against a
    /// copy of the prod AOF first.
    #[test]
    fn undeclared_fields_ride_along_without_disturbing_the_indexes() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        for i in 1..=3 {
            seed_starred(&st, "alice@x.com", &format!("t{i:02}"), 1000 + i);
        }

        // Payload the spec knows nothing about, on one of the rows.
        let key = keys::thread_user("alice@x.com", "t02");
        st.store()
            .hset(
                key.as_bytes(),
                &[
                    (b"count" as &[u8], b"7" as &[u8]),
                    (b"subject", "Undeclared".as_bytes()),
                ],
            )
            .unwrap();

        let got = st
            .list_thread_ids_by_flag_via_table("alice@x.com", "starred", 10, 0, None)
            .unwrap();
        assert_eq!(
            got,
            vec!["t03", "t02", "t01"],
            "an undeclared field must not drop the row out of its index"
        );

        let back = st.store().hget(key.as_bytes(), b"count").unwrap();
        assert_eq!(
            back.as_deref(),
            Some(b"7" as &[u8]),
            "and it must still be readable"
        );

        let verdict = st.store().table_verify_report(b"threaduser");
        assert!(verdict.is_ok(), "TABLE.VERIFY must stay happy: {verdict:?}");
    }

    /// If the sort were page-local, asking for 3 of 10 would return
    /// three arbitrary threads in descending order rather than the
    /// three newest — a paging bug that looks like correct output.
    #[test]
    fn flag_sort_is_global_not_page_local() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        // Insert oldest-first so a page-local sort would surface the
        // oldest three rather than the newest.
        for i in 1..=10 {
            seed_starred(&st, "alice@x.com", &format!("t{i:02}"), 1000 + i);
        }
        // A second user's starred threads must not appear.
        seed_starred(&st, "bob@y.com", "bobs", 9999);

        let got = st
            .list_thread_ids_by_flag_via_table("alice@x.com", "starred", 3, 0, None)
            .unwrap();
        assert_eq!(
            got,
            vec!["t10", "t09", "t08"],
            "SORT must order the whole match set, and FILTER must scope it to the user"
        );

        // And the page after it must continue, not restart.
        let next = st
            .list_thread_ids_by_flag_via_table("alice@x.com", "starred", 3, 3, None)
            .unwrap();
        assert_eq!(
            next,
            vec!["t07", "t06", "t05"],
            "OFFSET must page through the sorted set"
        );
    }
}
