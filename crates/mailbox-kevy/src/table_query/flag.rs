//! The boolean axes: starred, archived, pinned, unread, has_action and
//! the Sent flag.
//!
//! Each keys on its own small Range index and reaches the other
//! dimensions through the values stored beside it — `FILTER user` to
//! narrow, `SORT activity DESC` to order, and any further predicate as
//! one more filter. That is what keeps "starred and unread, within
//! Inbox" from needing an index of its own.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// Threads carrying one boolean flag, newest first, for one user.
    ///
    /// Keys on the flag's own Range index and reaches the other two
    /// dimensions through the values stored beside it: `FILTER user`
    /// narrows to the account, `SORT activity DESC` orders the result.
    /// That is what keeps five predicates to five small indexes rather
    /// than five more composites — but it also means the sort is a
    /// clause rather than the key's own order, so `flag_sort_is_global`
    /// pins the semantics.
    pub fn list_thread_ids_by_flag_via_table(
        &self,
        user: &str,
        flag: &str,
        limit: usize,
        offset: usize,
        before_ts: Option<i64>,
    ) -> io::Result<Vec<String>> {
        self.list_thread_ids_by_flag_filtered(user, flag, &[], limit, offset, before_ts)
    }

    /// The same axis with extra equality predicates applied by the
    /// engine.
    ///
    /// `extra` is `(column, value)` pairs over the columns stored
    /// beside the index. This is how a stacked filter — "archived
    /// within Inbox" — is answered from one index instead of an
    /// intersection of two.
    pub fn list_thread_ids_by_flag_filtered(
        &self,
        user: &str,
        flag: &str,
        extra: &[(&str, &str)],
        limit: usize,
        offset: usize,
        before_ts: Option<i64>,
    ) -> io::Result<Vec<String>> {
        use kevy_embedded::{IndexValue, ScalarQueryOpts, ValueFilter};
        let index = format!("threaduser.{flag}");
        let (lo, hi);
        let mut filters = vec![ValueFilter::Eq {
            field: b"user",
            value: user.as_bytes(),
        }];
        // The cursor is another FILTER on a stored value, not a key
        // range — the key here is the flag, not the timestamp.
        if let Some(ts) = before_ts {
            lo = i64::MIN.to_string();
            hi = ts.to_string();
            filters.push(ValueFilter::Range {
                field: b"activity",
                min: lo.as_bytes(),
                max: hi.as_bytes(),
            });
        }
        for (col, val) in extra {
            filters.push(ValueFilter::Eq {
                field: col.as_bytes(),
                value: val.as_bytes(),
            });
        }
        let page = self.store.idx_query_claused(
            index.as_bytes(),
            &IndexValue::I64(1),
            &IndexValue::I64(1),
            None,
            limit,
            ScalarQueryOpts {
                filters: &filters,
                sort: Some((b"activity", true)),
                distinct: None,
                facets: &[],
                offset,
            },
        )?;
        let prefix_len = keys::thread_user(user, "").len();
        Ok(page
            .rows
            .into_iter()
            .filter_map(|(key, _)| {
                let k = String::from_utf8(key).ok()?;
                k.get(prefix_len..).map(str::to_string)
            })
            .collect())
    }

    /// How many threads carry the flag for this user.
    ///
    /// `idx_count` takes no clauses, so this counts the rows a clause
    /// query returns. The flag axes are small in practice (starred,
    /// pinned, has_action are user-curated), and the cap bounds the
    /// worst case.
    pub fn count_thread_ids_by_flag_via_table(&self, user: &str, flag: &str) -> io::Result<usize> {
        self.count_thread_ids_by_flag_filtered(user, flag, &[])
    }

    /// Count on a flag axis with extra predicates applied.
    ///
    /// Counted by the engine, materializing nothing. This used to fetch up
    /// to 100,000 thread ids and take the length, because `idx_count` takes
    /// no clauses — kevy 5.1's `idx_count_claused` is that verb, and its own
    /// notes name this consumer shape: *"counting a filtered axis used to
    /// mean fetching every page and taking its length."*
    ///
    /// It is also more correct than what it replaces. The old cap silently
    /// became the answer for anyone past it, so an account with more than
    /// 100,000 unread threads was shown 100,000. There is no cap now.
    ///
    /// `count_equals_list_length` pins the two against each other, because
    /// the list applies one predicate this does not: it drops rows whose key
    /// does not start with the user's prefix, which `FILTER user` should
    /// already have excluded. If those two ever disagree the badge and the
    /// page would disagree, which is the failure this axis exists to avoid.
    pub fn count_thread_ids_by_flag_filtered(
        &self,
        user: &str,
        flag: &str,
        extra: &[(&str, &str)],
    ) -> io::Result<usize> {
        use kevy_embedded::{IndexValue, ValueFilter};
        let index = format!("threaduser.{flag}");
        let mut filters = vec![ValueFilter::Eq {
            field: b"user",
            value: user.as_bytes(),
        }];
        for (col, val) in extra {
            filters.push(ValueFilter::Eq {
                field: col.as_bytes(),
                value: val.as_bytes(),
            });
        }
        let n = self.store.idx_count_claused(
            index.as_bytes(),
            &IndexValue::I64(1),
            &IndexValue::I64(1),
            &filters,
        )?;
        Ok(usize::try_from(n).unwrap_or(usize::MAX))
    }
}
