//! The composite axes: bucket, category and plain recency.
//!
//! Each is one contiguous range over a declared ORDERPATH, which is what
//! makes both the page and its total cheap — the count is an index count
//! rather than a walk, and the cursor is a range on the component right
//! after the equality columns.

use std::io;

use super::ArchiveScope;
use crate::KevyMailboxStore;

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
        scope: ArchiveScope,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.query_orderpath(
            b"threaduser.by_user_bucket",
            user,
            b"bucket",
            bucket,
            scope,
            limit,
        )
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
        scope: ArchiveScope,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.query_orderpath(
            b"threaduser.by_user_category",
            user,
            b"category",
            category,
            scope,
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
        scope: ArchiveScope,
    ) -> io::Result<usize> {
        let (lo, hi) =
            self.bucket_bounds(b"threaduser.by_user_bucket", user, b"bucket", bucket, scope)?;
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
    /// `activity` is the component right after the equality columns in
    /// the composite, which is the only position a range may constrain
    /// — so this is the shape the declared ORDERPATH was designed to
    /// answer, not a scan with a filter on top. `scope` is one of those
    /// equality columns and must therefore be pinned; see
    /// [`ArchiveScope`].
    pub fn list_thread_ids_by_bucket_before_via_table(
        &self,
        user: &str,
        bucket: &str,
        scope: ArchiveScope,
        max_activity: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        use kevy_index::WhereClause;
        scope.require_pinned("list_thread_ids_by_bucket_before_via_table")?;
        let mut eqs = vec![
            (b"user".to_vec(), user.as_bytes().to_vec()),
            (b"bucket".to_vec(), bucket.as_bytes().to_vec()),
        ];
        scope.push_to(&mut eqs);
        let clause = WhereClause {
            eqs,
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
        scope: ArchiveScope,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.run_orderpath(
            b"threaduser.by_user_bucket_sent",
            user,
            &Self::unsent_clause(user, bucket, scope, None),
            limit,
        )
    }

    /// The cursor page of the same axis.
    pub fn list_thread_ids_by_bucket_unsent_before_via_table(
        &self,
        user: &str,
        bucket: &str,
        scope: ArchiveScope,
        max_activity: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        scope.require_pinned("list_thread_ids_by_bucket_unsent_before_via_table")?;
        self.run_orderpath(
            b"threaduser.by_user_bucket_sent",
            user,
            &Self::unsent_clause(user, bucket, scope, Some(max_activity)),
            limit,
        )
    }

    /// How many threads sit on the inbox-shaped axis.
    pub fn count_thread_ids_by_bucket_unsent_via_table(
        &self,
        user: &str,
        bucket: &str,
        scope: ArchiveScope,
    ) -> io::Result<usize> {
        let (lo, hi) = self.composite_bounds_for(
            b"threaduser.by_user_bucket_sent",
            &Self::unsent_clause(user, bucket, scope, None),
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
        scope: ArchiveScope,
        max_activity: Option<i64>,
    ) -> kevy_index::WhereClause {
        let mut eqs = vec![
            (b"user".to_vec(), user.as_bytes().to_vec()),
            (b"bucket".to_vec(), bucket.as_bytes().to_vec()),
            (b"sent_only".to_vec(), b"0".to_vec()),
        ];
        scope.push_to(&mut eqs);
        kevy_index::WhereClause {
            eqs,
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
        scope: ArchiveScope,
    ) -> io::Result<usize> {
        let (lo, hi) = self.bucket_bounds(
            b"threaduser.by_user_category",
            user,
            b"category",
            cat,
            scope,
        )?;
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
        scope: ArchiveScope,
        max_activity: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        scope.require_pinned("list_thread_ids_by_category_before_via_table")?;
        let mut eqs = vec![
            (b"user".to_vec(), user.as_bytes().to_vec()),
            (b"category".to_vec(), cat.as_bytes().to_vec()),
        ];
        scope.push_to(&mut eqs);
        let clause = kevy_index::WhereClause {
            eqs,
            range: Some((
                b"activity".to_vec(),
                i64::MIN.to_string().into_bytes(),
                max_activity.to_string().into_bytes(),
            )),
        };
        self.run_orderpath(b"threaduser.by_user_category", user, &clause, limit)
    }

    /// The default axis: one user's threads, newest first, no
    /// predicate.
    pub fn list_thread_ids_by_activity_via_table(
        &self,
        user: &str,
        scope: ArchiveScope,
        limit: usize,
        before_ts: Option<i64>,
    ) -> io::Result<Vec<String>> {
        if before_ts.is_some() {
            scope.require_pinned("list_thread_ids_by_activity_via_table")?;
        }
        let mut eqs = vec![(b"user".to_vec(), user.as_bytes().to_vec())];
        scope.push_to(&mut eqs);
        let clause = kevy_index::WhereClause {
            eqs,
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
    pub fn count_thread_ids_by_activity_via_table(
        &self,
        user: &str,
        scope: ArchiveScope,
    ) -> io::Result<usize> {
        let mut eqs = vec![(b"user".to_vec(), user.as_bytes().to_vec())];
        scope.push_to(&mut eqs);
        let clause = kevy_index::WhereClause { eqs, range: None };
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
}
