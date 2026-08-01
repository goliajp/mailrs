//! Reading the declared table — one function per axis the conversation
//! list can ask for.
//!
//! The composite encoding is what makes these cheap: `activity` sits right
//! after the equality columns, so a cursor is a range on the component the
//! ORDERPATH was designed to range over, not a scan with a filter on top.

use std::io;

use super::KevyMailboxStore;
use super::keys;
use super::table_spec::thread_user_spec;

impl KevyMailboxStore {
    /// Thread ids for one (user, bucket) pair, newest first, read from
    /// the declared ORDERPATH instead of a hand-maintained zset.
    ///
    /// `bucket` is the stored folder name (`inbox` / `notifications` /
    /// `promotions` / `junk`). Ordering comes from the composite
    /// encoding — `activity` is declared DESC, so the byte order the
    /// segment scans in is already the order the UI wants, and `ord`
    /// makes it total.
    pub fn list_thread_ids_by_bucket_via_table(
        &self,
        user: &str,
        bucket: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.query_orderpath(b"threaduser.by_user_bucket", user, b"bucket", bucket, limit)
    }

    /// Same, keyed on the message category rather than the folder.
    /// Every thread this user has a membership row for.
    ///
    /// The declared replacement for enumerating `user_threads_by_activity`,
    /// which is legacy: it is in [`keys::all_user_thread_zsets`], the set
    /// `drop-legacy-zsets` deletes, and nothing writes it any more. Measured
    /// on prod 2026-07-31 it held 168 rows across 13 accounts against 30,562
    /// declared ones, so every sweep that enumerated it had quietly stopped
    /// seeing the mailbox.
    ///
    /// A keyspace scan over the row prefix, which is what the census does
    /// over the same rows. That is acceptable for the callers this has —
    /// maintenance sweeps whose whole job is to visit every thread once —
    /// and it is not for a request path. There is no ORDERPATH that spans
    /// buckets, so the alternative would be a union over every bucket, which
    /// costs the same and can miss a bucket nobody remembered to list.
    pub fn all_thread_ids_for_user(&self, user: &str) -> io::Result<Vec<String>> {
        let prefix = format!("mailrs:threaduser:{user}:");
        Ok(self
            .store()
            .keys(Some(format!("{prefix}*").as_bytes()), None)
            .into_iter()
            .filter_map(|k| {
                let k = String::from_utf8(k).ok()?;
                k.strip_prefix(&prefix).map(str::to_string)
            })
            .collect())
    }

    pub fn list_thread_ids_by_category_via_table(
        &self,
        user: &str,
        category: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.query_orderpath(
            b"threaduser.by_user_category",
            user,
            b"category",
            category,
            limit,
        )
    }

    /// How many threads sit on one (user, bucket) axis.
    ///
    /// Counts in the index rather than materialising keys — the page
    /// total on a 1400-row axis should not cost 1400 key copies.
    pub fn count_thread_ids_by_bucket_via_table(
        &self,
        user: &str,
        bucket: &str,
    ) -> io::Result<usize> {
        let (lo, hi) = self.bucket_bounds(b"threaduser.by_user_bucket", user, b"bucket", bucket)?;
        self.store
            .idx_count(
                b"threaduser.by_user_bucket",
                &kevy_embedded::IndexValue::Str(lo),
                &kevy_embedded::IndexValue::Str(hi),
            )
            .map(|n| n as usize)
            .map_err(io::Error::other)
    }

    /// The cursor page: threads on this axis older than `max_activity`.
    ///
    /// `activity` is the component right after the two equality columns
    /// in the composite, which is the only position a range may
    /// constrain — so this is the shape the declared ORDERPATH was
    /// designed to answer, not a scan with a filter on top.
    pub fn list_thread_ids_by_bucket_before_via_table(
        &self,
        user: &str,
        bucket: &str,
        max_activity: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        use kevy_index::WhereClause;
        let clause = WhereClause {
            eqs: vec![
                (b"user".to_vec(), user.as_bytes().to_vec()),
                (b"bucket".to_vec(), bucket.as_bytes().to_vec()),
            ],
            range: Some((
                b"activity".to_vec(),
                i64::MIN.to_string().into_bytes(),
                max_activity.to_string().into_bytes(),
            )),
        };
        self.run_orderpath(b"threaduser.by_user_bucket", user, &clause, limit)
    }

    /// Inbox-shaped read: one bucket, excluding threads the user only
    /// ever sent. Newest first.
    pub fn list_thread_ids_by_bucket_unsent_via_table(
        &self,
        user: &str,
        bucket: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.run_orderpath(
            b"threaduser.by_user_bucket_sent",
            user,
            &Self::unsent_clause(user, bucket, None),
            limit,
        )
    }

    /// The cursor page of the same axis.
    pub fn list_thread_ids_by_bucket_unsent_before_via_table(
        &self,
        user: &str,
        bucket: &str,
        max_activity: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.run_orderpath(
            b"threaduser.by_user_bucket_sent",
            user,
            &Self::unsent_clause(user, bucket, Some(max_activity)),
            limit,
        )
    }

    /// How many threads sit on the inbox-shaped axis.
    pub fn count_thread_ids_by_bucket_unsent_via_table(
        &self,
        user: &str,
        bucket: &str,
    ) -> io::Result<usize> {
        let (lo, hi) = self.composite_bounds_for(
            b"threaduser.by_user_bucket_sent",
            &Self::unsent_clause(user, bucket, None),
        )?;
        self.store
            .idx_count(
                b"threaduser.by_user_bucket_sent",
                &kevy_embedded::IndexValue::Str(lo),
                &kevy_embedded::IndexValue::Str(hi),
            )
            .map(|n| n as usize)
            .map_err(io::Error::other)
    }

    fn unsent_clause(
        user: &str,
        bucket: &str,
        max_activity: Option<i64>,
    ) -> kevy_index::WhereClause {
        kevy_index::WhereClause {
            eqs: vec![
                (b"user".to_vec(), user.as_bytes().to_vec()),
                (b"bucket".to_vec(), bucket.as_bytes().to_vec()),
                (b"sent_only".to_vec(), b"0".to_vec()),
            ],
            range: max_activity.map(|ts| {
                (
                    b"activity".to_vec(),
                    i64::MIN.to_string().into_bytes(),
                    ts.to_string().into_bytes(),
                )
            }),
        }
    }

    /// How many threads currently carry this category.
    pub fn count_thread_ids_by_category_via_table(
        &self,
        user: &str,
        cat: &str,
    ) -> io::Result<usize> {
        let (lo, hi) =
            self.bucket_bounds(b"threaduser.by_user_category", user, b"category", cat)?;
        self.store
            .idx_count(
                b"threaduser.by_user_category",
                &kevy_embedded::IndexValue::Str(lo),
                &kevy_embedded::IndexValue::Str(hi),
            )
            .map(|n| n as usize)
            .map_err(io::Error::other)
    }

    /// The cursor page of the category axis.
    pub fn list_thread_ids_by_category_before_via_table(
        &self,
        user: &str,
        cat: &str,
        max_activity: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        let clause = kevy_index::WhereClause {
            eqs: vec![
                (b"user".to_vec(), user.as_bytes().to_vec()),
                (b"category".to_vec(), cat.as_bytes().to_vec()),
            ],
            range: Some((
                b"activity".to_vec(),
                i64::MIN.to_string().into_bytes(),
                max_activity.to_string().into_bytes(),
            )),
        };
        self.run_orderpath(b"threaduser.by_user_category", user, &clause, limit)
    }

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
        let page = self
            .store
            .idx_query_claused(
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
            )
            .map_err(io::Error::other)?;
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
    pub fn count_thread_ids_by_flag_filtered(
        &self,
        user: &str,
        flag: &str,
        extra: &[(&str, &str)],
    ) -> io::Result<usize> {
        self.list_thread_ids_by_flag_filtered(user, flag, extra, 100_000, 0, None)
            .map(|v| v.len())
    }

    /// The default axis: one user's threads, newest first, no
    /// predicate.
    pub fn list_thread_ids_by_activity_via_table(
        &self,
        user: &str,
        limit: usize,
        before_ts: Option<i64>,
    ) -> io::Result<Vec<String>> {
        let clause = kevy_index::WhereClause {
            eqs: vec![(b"user".to_vec(), user.as_bytes().to_vec())],
            range: before_ts.map(|ts| {
                (
                    b"activity".to_vec(),
                    i64::MIN.to_string().into_bytes(),
                    ts.to_string().into_bytes(),
                )
            }),
        };
        self.run_orderpath(b"threaduser.by_user_activity", user, &clause, limit)
    }

    /// How many threads the user has in total.
    pub fn count_thread_ids_by_activity_via_table(&self, user: &str) -> io::Result<usize> {
        let clause = kevy_index::WhereClause {
            eqs: vec![(b"user".to_vec(), user.as_bytes().to_vec())],
            range: None,
        };
        let (lo, hi) = self.composite_bounds_for(b"threaduser.by_user_activity", &clause)?;
        self.store
            .idx_count(
                b"threaduser.by_user_activity",
                &kevy_embedded::IndexValue::Str(lo),
                &kevy_embedded::IndexValue::Str(hi),
            )
            .map(|n| n as usize)
            .map_err(io::Error::other)
    }

    fn bucket_bounds(
        &self,
        index: &[u8],
        user: &str,
        second_col: &[u8],
        second_val: &str,
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        use kevy_index::WhereClause;
        let clause = WhereClause {
            eqs: vec![
                (b"user".to_vec(), user.as_bytes().to_vec()),
                (second_col.to_vec(), second_val.as_bytes().to_vec()),
            ],
            range: None,
        };
        self.composite_bounds_for(index, &clause)
    }

    fn query_orderpath(
        &self,
        index: &[u8],
        user: &str,
        second_col: &[u8],
        second_val: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        use kevy_index::WhereClause;
        let clause = WhereClause {
            eqs: vec![
                (b"user".to_vec(), user.as_bytes().to_vec()),
                (second_col.to_vec(), second_val.as_bytes().to_vec()),
            ],
            range: None,
        };
        self.run_orderpath(index, user, &clause, limit)
    }

    /// Resolve the composite columns for `index` from the declared
    /// spec, so the bounds are always encoded the way the segment was.
    fn composite_bounds_for(
        &self,
        index: &[u8],
        clause: &kevy_index::WhereClause,
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        use kevy_index::{CompositeCol, composite_bounds};
        let spec = thread_user_spec();
        let path = spec
            .orderpaths
            .iter()
            .find(|p| index.ends_with(&p.name))
            .ok_or_else(|| io::Error::other("no such orderpath in the spec"))?;
        let cols: Vec<CompositeCol> = path
            .on
            .iter()
            .map(|(col, desc)| {
                let ty = spec
                    .column_type(col)
                    .ok_or_else(|| io::Error::other("orderpath names an undeclared column"))?;
                Ok(CompositeCol {
                    name: col.clone(),
                    ty,
                    desc: *desc,
                })
            })
            .collect::<io::Result<_>>()?;
        composite_bounds(&cols, clause).map_err(io::Error::other)
    }

    fn run_orderpath(
        &self,
        index: &[u8],
        user: &str,
        clause: &kevy_index::WhereClause,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        let (lo, hi) = self.composite_bounds_for(index, clause)?;
        let (rows, _cursor) = self
            .store
            .idx_query(
                index,
                &kevy_embedded::IndexValue::Str(lo),
                &kevy_embedded::IndexValue::Str(hi),
                None,
                limit,
            )
            .map_err(io::Error::other)?;

        // The row key is `mailrs:threaduser:{user}:{tid}`; the tid can
        // itself contain colons (it is a Message-ID), so split off the
        // known prefix rather than splitting on the separator.
        let prefix_len = keys::thread_user(user, "").len();
        Ok(rows
            .into_iter()
            .filter_map(|(key, _)| {
                let k = String::from_utf8(key).ok()?;
                k.get(prefix_len..).map(str::to_string)
            })
            .collect())
    }
}
