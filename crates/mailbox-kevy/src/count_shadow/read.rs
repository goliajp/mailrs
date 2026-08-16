//! Reading the counts the engine keeps, and the cutover that made them
//! what a page shows.
//!
//! A submodule so it can reach the shadow's test helpers and the parent's
//! private items — two siblings would see neither's. Split out under the
//! 500-line rule, which refused a deploy at 539; the baseline is empty and
//! is meant to stay that way.

use super::super::KevyMailboxStore;
use super::super::keys;
use crate::messages::State;

impl KevyMailboxStore {
    /// One thread's three counters, counted by the engine.
    ///
    /// `None` when the index cannot answer — a row that predates the group
    /// column is invisible to it, and a page must not silently show zero
    /// for a thread that simply has not been backfilled. The caller keeps
    /// the stored numbers in that case.
    ///
    /// **Must not be called inside `store.atomic`.** `idx_group` takes a
    /// write lock on every shard to sync its segments; the page hydration
    /// holds its own atomic block, so the counters are read after it
    /// rather than inside it.
    pub fn counts_from_index(&self, user: &str, thread_id: &str) -> Option<(i64, i64, i64)> {
        let g = |st: State| -> Option<u64> {
            self.store()
                .idx_group(
                    keys::IDX_USERMSG_COUNTS,
                    crate::messages::group_name(user, thread_id, st).as_bytes(),
                )
                .ok()
                .map(|s| s.count)
        };
        let (unread, read, own) = (g(State::Unread)?, g(State::Read)?, g(State::Own)?);
        let total = unread + read + own;
        // A thread the index has never seen answers zero on all three, and
        // zero is also what an empty thread answers. Refusing to speak for
        // a thread with no indexed rows is what keeps a page from showing
        // an un-backfilled conversation as empty.
        match total {
            0 => None,
            _ => Some((total as i64, unread as i64, own as i64)),
        }
    }
}

#[cfg(test)]
mod read_tests {
    use super::*;
    use crate::count_shadow::tests::*;

    /// The engine answers a backfilled thread, and refuses one it has
    /// never seen — because zero from the index and zero from an empty
    /// thread are the same number, and only one of them is an answer.
    #[test]
    fn the_index_refuses_to_speak_for_a_thread_it_cannot_see() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        assert_eq!(s.counts_from_index(u, "t1"), Some((1, 1, 0)));

        // A thread whose row predates the column.
        s.store()
            .hdel(
                keys::user_message(u, "m1").as_bytes(),
                &[keys::USER_MESSAGE_GROUP_FIELD],
            )
            .unwrap();
        assert_eq!(
            s.counts_from_index(u, "t1"),
            None,
            "an un-backfilled thread must not be answered as empty"
        );
        assert_eq!(s.counts_from_index(u, "never-existed"), None);
    }

    /// Own sends count toward sent and never toward unread.
    #[test]
    fn the_three_numbers_are_the_three_the_row_carries() {
        let s = store();
        let u = "u@x.com";
        for _ in 0..3 {
            arrive(&s, "t1", u, true, false);
        }
        put(&s, u, "t1", "m1", 0, "other@z.com");
        put(&s, u, "t1", "m2", 1, "other@z.com");
        put(&s, u, "t1", "m3", 0, "u@x.com");
        assert_eq!(s.counts_from_index(u, "t1"), Some((3, 1, 1)));
    }
}

#[cfg(test)]
mod cutover_tests {
    use super::*;
    use crate::ListThreadsFilter;
    use crate::count_shadow::tests::*;

    /// **The page shows the engine's number, not the stored one.**
    ///
    /// The whole point of the move: a stored counter that drifts stops
    /// being what anybody sees. A3 found 127 threads on production whose
    /// stored counts disagreed with their messages, written by four
    /// different callers between them — this makes that class of defect
    /// invisible to the user rather than merely repairable.
    #[test]
    fn a_page_reads_the_count_from_the_index_and_not_the_row() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");

        // Drift the stored side the way a missed writer would.
        s.store()
            .hset(
                keys::thread_user(u, "t1").as_bytes(),
                &[
                    (b"count".as_slice(), b"99".as_slice()),
                    (b"unread_count".as_slice(), b"99".as_slice()),
                    (b"sent_count".as_slice(), b"99".as_slice()),
                ],
            )
            .unwrap();

        let (rows, _) = s
            .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 50)
            .unwrap();
        let row = rows
            .iter()
            .find(|r| r.thread_id == "t1")
            .expect("the thread");
        assert_eq!(
            (row.count, row.unread_count, row.sent_count),
            (1, 1, 0),
            "the page served the drifted row instead of the engine's count"
        );
    }

    /// **A single-thread read answers from the index too, not only a page.**
    ///
    /// `hydrate_page` moved in C3, but `get_thread_for_user` did not — and
    /// it is what search-scope filtering, `mark_read`'s was-unread gate,
    /// `mark_list_read`'s counter and `archive_thread`'s dismissed-unread
    /// gate all read. Leaving it on the stored row means the counters must
    /// keep being written, so this is the step that makes retiring them
    /// possible: readers first, writers after.
    #[test]
    fn a_single_thread_read_answers_from_the_index() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");

        s.store()
            .hset(
                keys::thread_user(u, "t1").as_bytes(),
                &[
                    (b"count".as_slice(), b"99".as_slice()),
                    (b"unread_count".as_slice(), b"99".as_slice()),
                    (b"sent_count".as_slice(), b"99".as_slice()),
                ],
            )
            .unwrap();

        let row = s.get_thread_for_user(u, "t1").unwrap().expect("the row");
        assert_eq!(
            (row.count, row.unread_count, row.sent_count),
            (1, 1, 0),
            "get_thread_for_user served the drifted row"
        );
    }

    /// **A thread the index cannot see now counts zero, and that is the
    /// cost of C5b-2 rather than a defect.**
    ///
    /// This used to assert the opposite: the stored numbers stood in, so
    /// an un-backfilled conversation still showed its counts. Nothing
    /// writes those numbers any more, so there is nothing to stand in
    /// with, and `counts_from_index` returning `None` leaves the row's
    /// zeros.
    ///
    /// Pinned rather than removed because the exposure is real and worth
    /// being able to see: a row with no group column renders as an empty
    /// conversation. What keeps it out of production is that the
    /// backfill is complete — `rows_ungrouped_store_wide: 0` across all
    /// thirteen accounts, re-checked after every deploy in this phase —
    /// and that `upsert_user_message` writes the column on every row it
    /// creates, so no new row can arrive without one.
    ///
    /// If this ever needs to be safe rather than merely true, the answer
    /// is `maintenance:group-backfill`, not a second copy of the counts.
    #[test]
    fn a_thread_the_index_cannot_see_counts_zero() {
        let s = store();
        let u = "u@x.com";
        arrive(&s, "t1", u, true, false);
        put(&s, u, "t1", "m1", 0, "other@z.com");
        s.store()
            .hdel(
                keys::user_message(u, "m1").as_bytes(),
                &[keys::USER_MESSAGE_GROUP_FIELD],
            )
            .unwrap();

        let (rows, _) = s
            .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 50)
            .unwrap();
        let row = rows
            .iter()
            .find(|r| r.thread_id == "t1")
            .expect("the thread");
        assert_eq!(
            (row.count, row.unread_count),
            (0, 0),
            "a row without a group column has nothing left to count it"
        );
        // And the debt is visible where the backfill can act on it.
        assert_eq!(s.ungrouped_user_message_rows().unwrap(), (1, 1));
    }
}
