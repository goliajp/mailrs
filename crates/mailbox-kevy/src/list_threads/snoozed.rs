//! The Snoozed list: this reader's conversations put away until a time
//! still to come.
//!
//! Read off the `snoozed_until` index rather than an ORDERPATH. A snoozed
//! row is archived while it is away, so no declared list shape holds
//! "away and not yet due" — and filtering the Archived page would leave its
//! total counting rows the page does not show. The index already exists
//! for waking threads, and snoozed rows are few by nature, so the list is
//! read whole and its total is its length.

use std::io;

use kevy_embedded::IndexValue;

use super::{ListThreadsFilter, QuarantineScope};
use crate::KevyMailboxStore;
use crate::thread_row::ThreadRow;

impl KevyMailboxStore {
    /// `user`'s conversations snoozed past `now`, newest activity first.
    /// Trashed rows never show; held ones follow `filter.quarantine`, and
    /// `filter.accounts` narrows as on every list. The folder does not apply.
    pub fn list_snoozed(
        &self,
        user: &str,
        now: i64,
        filter: &ListThreadsFilter<'_>,
    ) -> io::Result<Vec<ThreadRow>> {
        let prefix = crate::keys::thread_user(user, "");
        let mut keys = Vec::new();
        let mut cursor = None;
        loop {
            let (hits, next) = self.store().idx_query(
                crate::thread_row::index_for("snoozed_until").as_bytes(),
                &IndexValue::I64(now.saturating_add(1)),
                &IndexValue::I64(i64::MAX),
                cursor.as_ref(),
                10_000,
            )?;
            keys.extend(hits.into_iter().map(|(key, _)| key));
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        let mut rows = Vec::new();
        for key in keys {
            let key = String::from_utf8(key).map_err(io::Error::other)?;
            let Some(tid) = key.strip_prefix(&prefix) else {
                continue;
            };
            let Some(row) = self.get_thread_for_user(user, tid)? else {
                continue;
            };
            if row.trashed_at > 0 || !shown(&row, filter.quarantine) {
                continue;
            }
            if filter
                .accounts
                .as_ref()
                .is_some_and(|ids| !ids.contains(&row.account_id))
            {
                continue;
            }
            rows.push(row);
        }
        rows.sort_by(|a, b| {
            b.latest_date
                .cmp(&a.latest_date)
                .then_with(|| a.thread_id.cmp(&b.thread_id))
        });
        Ok(rows)
    }
}

fn shown(row: &ThreadRow, quarantine: QuarantineScope) -> bool {
    match quarantine {
        QuarantineScope::Hide => !row.quarantined,
        QuarantineScope::Include => true,
        QuarantineScope::Only => row.quarantined,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kevy_embedded::{Config, Store};

    use crate::KevyMailboxStore;
    use crate::list_threads::ListThreadsFilter;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        s.ensure_thread_table();
        s
    }

    fn arrive(st: &KevyMailboxStore, user: &str, tid: &str, at: i64) {
        st.record_message_arrival(&crate::MessageArrival {
            thread_id: tid,
            user,
            subject: "s",
            senders_csv: "a@x.com",
            latest_date: at,
            latest_preview: "p",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
    }

    #[test]
    fn lists_what_is_away_past_now_for_this_reader_only() {
        let st = store();
        let (alice, bob) = ("alice@x.com", "bob@x.com");
        for (tid, at) in [("later", 100), ("due", 200), ("never", 300), ("newer", 400)] {
            arrive(&st, alice, tid, at);
        }
        arrive(&st, bob, "bobs", 500);
        st.set_snoozed(alice, "later", 2_000).unwrap();
        st.set_snoozed(alice, "newer", 3_000).unwrap();
        st.set_snoozed(alice, "due", 900).unwrap();
        st.set_snoozed(bob, "bobs", 2_000).unwrap();

        let rows = st
            .list_snoozed(alice, 1_000, &ListThreadsFilter::default())
            .unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.thread_id.as_str()).collect();
        // newest activity first; the one whose time has passed and bob's are not here
        assert_eq!(ids, ["newer", "later"]);
    }

    #[test]
    fn a_trashed_or_woken_thread_is_not_snoozed() {
        let st = store();
        let u = "alice@x.com";
        arrive(&st, u, "a", 100);
        arrive(&st, u, "b", 200);
        st.set_snoozed(u, "a", 2_000).unwrap();
        st.set_snoozed(u, "b", 2_000).unwrap();
        st.set_snoozed(u, "b", 0).unwrap();
        st.set_trashed(u, "a", true, 1_500).unwrap();
        let rows = st
            .list_snoozed(u, 1_000, &ListThreadsFilter::default())
            .unwrap();
        assert!(
            rows.is_empty(),
            "{:?}",
            rows.iter().map(|r| &r.thread_id).collect::<Vec<_>>()
        );
    }
}
