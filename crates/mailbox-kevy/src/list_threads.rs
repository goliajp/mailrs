//! `list_threads_by_activity` — Rock 1 cascade-killer's real exit.
//!
//! Replaces the SQL aggregate (`string_agg DISTINCT` + 3 correlated
//! subqueries + BOOL_OR + COUNT DISTINCT CASE) with one ZREVRANGE on the
//! per-user activity zset followed by N × HGETALL on each thread hash.
//! Total cost: O(log n + N) instead of O(rows × messages).
//!
//! Filtering by category / archived / pinned / has_unread / has_action
//! uses the matching secondary zset (same shape, intersected with
//! activity score range).

use std::io;

use super::KevyMailboxStore;
use super::keys;
use super::thread_row::ThreadRow;

/// Filter knobs passed to `list_threads_by_activity`. None of these are
/// required; default is "all threads sorted by recency, latest first."
#[derive(Debug, Clone, Default)]
pub struct ListThreadsFilter<'a> {
    /// Restrict to a single category (`inbox`, `social`, etc.). When
    /// set, the activity zset is replaced with the per-category index.
    pub category: Option<&'a str>,
    /// Match monolith's `folder` query. `Some("Sent")` (case-insensitive)
    /// flips the source index to the sent zset. Anything else falls
    /// through to the default axis.
    pub folder: Option<&'a str>,
    /// Only threads with `pinned = true`. Implemented as ZREVRANGE on
    /// the pinned index.
    pub pinned: bool,
    /// Only threads with `archived = true`. Likewise — archived index.
    pub archived: bool,
    /// Only threads with `unread_count > 0`. Uses the has_unread index.
    pub has_unread: bool,
    /// Only threads with `has_action = true`. Uses the has_action index.
    pub has_action: bool,
    /// Only threads with `starred = true`. Uses the starred index.
    pub starred: bool,
    /// Cursor for pagination: only return threads with `latest_date <
    /// before_ts`. Enables O(log n) load-more via ZREVRANGEBYSCORE.
    /// When `None`, the caller controls window via `(offset, limit)`.
    pub before_ts: Option<i64>,
}

impl<'a> ListThreadsFilter<'a> {
    /// Enumerate the index keys the current filter requires. When only
    /// one predicate is set, the returned Vec has a single entry and
    /// callers can use it directly. When ≥ 2 are set (e.g. inbox ∩
    /// has_unread), callers must ZINTERSTORE the collected keys and
    /// read the intersection.
    ///
    /// `folder = Sent | Junk | Inbox` is treated as an axis switch, not
    /// a predicate stacked on top of the others — matches the monolith's
    /// semantics. Sent + Junk + Inbox each resolve to their dedicated
    /// zset (v2.4.0 roadmap Phase 2, RFC-A).
    fn predicate_index_keys(&self, user: &str) -> Vec<String> {
        if let Some(f) = self.folder {
            if f.eq_ignore_ascii_case("sent") {
                return vec![keys::user_threads_sent(user)];
            }
            if f.eq_ignore_ascii_case("junk") {
                // v2.4.0 Phase 2 (RFC-A) — Junk folder read path.
                // Dedicated `user_threads_junk` zset is authoritative.
                // Every new arrival with category=="spam" fires an
                // upsert_thread that ZADDs both this zset and (for
                // legacy compat) `by_category:spam` in a single atomic
                // closure — so post-cutover the two are always in
                // sync. Pre-cutover threads only exist in
                // `by_category:spam`; the deploy runbook runs a
                // one-shot `scripts/backfill-junk-index.sh` to copy
                // them into `user_threads_junk`.
                return vec![keys::user_threads_junk(user)];
            }
            if f.eq_ignore_ascii_case("notifications") {
                // v2.9 triage — Notifications bucket, pure axis switch.
                return vec![keys::user_threads_notifications(user)];
            }
            if f.eq_ignore_ascii_case("promotions") {
                // v2.9 triage — Promotions bucket, pure axis switch.
                return vec![keys::user_threads_promotions(user)];
            }
            // "np" (the merged N & P view) is the UNION of the two
            // bucket zsets — handled specially in
            // `list_threads_by_activity` via ZUNIONSTORE, not here (this
            // function's multi-key return is ZINTERSTORE'd). See
            // `np_union_keys`.
            if f.eq_ignore_ascii_case("inbox") {
                // Inbox axis + additional predicates below stack via
                // ZINTERSTORE — same shape as any other multi-index
                // path. Push the Inbox zset first and fall through.
                let mut out: Vec<String> = Vec::with_capacity(4);
                out.push(keys::user_threads_inbox(user));
                if let Some(cat) = self.category {
                    out.push(keys::user_threads_by_category(user, cat));
                }
                if self.pinned {
                    out.push(keys::user_threads_pinned(user));
                }
                if self.archived {
                    out.push(keys::user_threads_archived(user));
                }
                if self.has_unread {
                    out.push(keys::user_threads_has_unread(user));
                }
                if self.has_action {
                    out.push(keys::user_threads_has_action(user));
                }
                if self.starred {
                    out.push(keys::user_threads_starred(user));
                }
                return out;
            }
        }
        let mut out: Vec<String> = Vec::with_capacity(4);
        if let Some(cat) = self.category {
            out.push(keys::user_threads_by_category(user, cat));
        }
        if self.pinned {
            out.push(keys::user_threads_pinned(user));
        }
        if self.archived {
            out.push(keys::user_threads_archived(user));
        }
        if self.has_unread {
            out.push(keys::user_threads_has_unread(user));
        }
        if self.has_action {
            out.push(keys::user_threads_has_action(user));
        }
        if self.starred {
            out.push(keys::user_threads_starred(user));
        }
        if out.is_empty() {
            out.push(keys::user_threads_by_activity(user));
        }
        out
    }

    fn pick_index_key(&self, user: &str) -> String {
        // Kept for callers that only need a single index key (e.g. the
        // score-range zrevrange path). Multi-predicate callers should
        // use predicate_index_keys() + ZINTERSTORE.
        self.predicate_index_keys(user).remove(0)
    }

    /// The merged "N & P" view — `Some([notifications, promotions])`
    /// when `folder == "np"`, else `None`. These are ZUNIONSTORE'd (a
    /// union, not the intersection `predicate_index_keys` produces).
    fn np_union_keys(&self, user: &str) -> Option<Vec<String>> {
        match self.folder {
            Some(f) if f.eq_ignore_ascii_case("np") => Some(vec![
                keys::user_threads_notifications(user),
                keys::user_threads_promotions(user),
            ]),
            _ => None,
        }
    }
}

impl KevyMailboxStore {
    /// List threads for `user` in reverse-activity order, with optional
    /// filter. `offset` skips the first N matches; `limit` caps the
    /// returned row count.
    ///
    /// Returns `(rows, total_in_index)`. `total_in_index` is the
    /// pre-pagination count of the chosen index — exactly the
    /// "X / Y conversations" badge the UI shows.
    pub fn list_threads_by_activity(
        &self,
        user: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        // Junk read cutover (kevy v4 TABLE migration). The declared
        // ORDERPATH replaces `user_threads_junk` for this axis.
        //
        // A shadow read over all 12 accounts showed the zset holding a
        // fraction of what the rows say — one account at 166 of 1456,
        // four others empty against non-empty rows — because
        // maintaining it by hand meant a write path could forget an
        // axis with nothing to catch it. Serving the table also fixes
        // that, so this changes what users see: threads already judged
        // spam start appearing in Junk.
        //
        // Set MAILRS_JUNK_READ=zset to serve the old axis again; no
        // rebuild is needed since both are still maintained on write.
        if let Some(bucket) = filter.bare_bucket()
            && bucket_reads_table()
        {
            return self.list_bucket_via_table(user, bucket, filter, offset, limit);
        }

        // The category axis has the opposite drift from the bucket
        // axes: its zsets accumulate. Nothing removes a thread from
        // `by_category:inbox` when it is reclassified, so on prod that
        // key held 28598 entries against 6787 live rows — 76% of it
        // was threads that had moved elsewhere. The rows carry the
        // current verdict, so this both cuts over and corrects.
        if let Some(cat) = filter.bare_category()
            && bucket_reads_table()
        {
            return self.list_category_via_table(user, cat, filter, offset, limit);
        }

        // The boolean predicates. Each keys on its own flag index and
        // reaches user + recency through the values stored beside it,
        // which is why five predicates cost five small indexes rather
        // than five more composites.
        if let Some(flag) = filter.bare_flag()
            && bucket_reads_table()
        {
            return self.list_flag_via_table(user, flag, filter, offset, limit);
        }

        // A folder or category with a flag stacked on it. The zsets
        // answered this with ZINTERSTORE; one index answers it now,
        // because every column the other predicate needs is stored
        // beside the flag. Without this the whole class — "archived
        // within Inbox", "unread within Inbox" — falls through to a
        // path that no longer has any index to read.
        if bucket_reads_table()
            && let Some((flag, scope)) = filter.stacked_predicate()
        {
            return self.list_stacked_via_table(user, flag, scope, filter, offset, limit);
        }

        // Sent is the sent_only flag; np is the union of two bucket
        // axes, merged here because an ORDERPATH answers one range and
        // a union is two.
        if bucket_reads_table() {
            // Sent was held back for a round: one account's zset held
            // 58 threads where the rows said 9. The cause was not the
            // predicate but the backfill's source — those threads had
            // no membership row at all, because the backfill walked
            // by_activity and that zset was missing them. Walking the
            // union of every legacy zset wrote the 49, and the axis
            // now agrees exactly.
            if filter.is_bare_sent() {
                return self.list_flag_via_table(user, "is_sender", filter, offset, limit);
            }
            if filter.is_bare_np() {
                return self.list_np_via_table(user, filter, offset, limit);
            }
            if filter.is_bare_default() {
                return self.list_default_via_table(user, filter, offset, limit);
            }
        }

        // v2 Stage B.4/B.6: kevy 3.17 ships ZINTERSTORE — when the
        // caller stacks ≥ 2 predicates (e.g. inbox ∩ has_unread),
        // materialize the intersection into a per-request temp zset
        // scored by the max latest_date. Prior implementation walked
        // the highest-priority single index and let the UI show
        // over-count badges. The temp key is TTL-tagged so an orphan
        // (e.g. panic mid-request) auto-cleans.
        // v2.9 — the merged "N & P" view is a ZUNIONSTORE of the two
        // bucket zsets into a per-request temp key (mirrors the
        // ZINTERSTORE temp-key pattern below; TTL-tagged so an orphan
        // auto-cleans). Handled before the intersection path because a
        // union has different algebra.
        let index_keys: Vec<String>;
        let owned_temp: Option<String>;
        if let Some(union_keys) = filter.np_union_keys(user) {
            let ts_nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let temp = format!("mailrs:tmp:zunion:{user}:{ts_nanos}");
            let refs: Vec<&[u8]> = union_keys.iter().map(|k| k.as_bytes()).collect();
            self.store()
                .zunionstore(temp.as_bytes(), &refs, None, kevy_embedded::ZAggregate::Max)
                .map_err(std::io::Error::other)?;
            self.store()
                .expire(temp.as_bytes(), std::time::Duration::from_secs(60))
                .map_err(std::io::Error::other)?;
            index_keys = vec![temp.clone()];
            owned_temp = Some(temp);
        } else {
            index_keys = filter.predicate_index_keys(user);
            owned_temp = if index_keys.len() > 1 {
                let ts_nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let temp = format!("mailrs:tmp:zinter:{user}:{ts_nanos}");
                let refs: Vec<&[u8]> = index_keys.iter().map(|k| k.as_bytes()).collect();
                self.store()
                    .zinterstore(temp.as_bytes(), &refs, None, kevy_embedded::ZAggregate::Max)
                    .map_err(std::io::Error::other)?;
                self.store()
                    .expire(temp.as_bytes(), std::time::Duration::from_secs(60))
                    .map_err(std::io::Error::other)?;
                Some(temp)
            } else {
                None
            };
        }
        let key: &str = owned_temp
            .as_deref()
            .unwrap_or_else(|| index_keys[0].as_str());
        let total = self
            .store()
            .zcard(key.as_bytes())
            .map_err(std::io::Error::other)?;
        if limit == 0 {
            return Ok((Vec::new(), total));
        }

        // Cursor branch — used by "load more". `before_ts` is the
        // `last_date` of the previous page's tail; return threads with
        // strictly smaller latest_date, ordered by score descending.
        // kevy's `zrev_range_by_score` doesn't take a LIMIT, so we
        // slice manually. For an in-memory-backed store this is fine
        // up to ~100k entries; a future kevy release with LIMIT can
        // replace the take().
        let entries = if let Some(ts) = filter.before_ts {
            let max = (ts - 1) as f64;
            let raw = self
                .store()
                .zrev_range_by_score(key.as_bytes(), max, f64::NEG_INFINITY)
                .map_err(std::io::Error::other)?;
            raw.into_iter().take(limit).collect()
        } else {
            if offset >= total {
                return Ok((Vec::new(), total));
            }
            let stop_exclusive = offset + limit;
            let stop_inclusive_idx = (stop_exclusive.min(total) as i64) - 1;
            self.store()
                .zrevrange(key.as_bytes(), offset as i64, stop_inclusive_idx)
                .map_err(std::io::Error::other)?
        };
        // v2 Stage B.3: fetch the N thread hashes inside one atomic
        // closure so the whole page assembles under a single shard
        // write lock — no interleaving writer can shift a row's
        // flags/counters between hgetalls. The initial zcard +
        // zrevrange stay outside the closure because AtomicCtx has
        // no zset reads in kevy 3.17.
        let result = self.store().atomic(|ctx| {
            let mut out = Vec::with_capacity(entries.len());
            for (tid_bytes, _score) in &entries {
                let Ok(tid) = std::str::from_utf8(tid_bytes) else {
                    continue;
                };
                let hkey = keys::thread(tid);
                let pairs = ctx.hgetall(hkey.as_bytes())?;
                if let Some(row) = ThreadRow::from_pairs(tid.to_string(), &pairs) {
                    out.push(row);
                }
            }
            Ok((out, total))
        });
        // Reclaim the intersection temp promptly; TTL is a fallback
        // for a mid-request panic, not the primary GC path.
        if let Some(temp) = owned_temp {
            let _ = self.store().del(&[temp.as_bytes()]);
        }
        result.map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        // Reads are served from the declared table, so a test store
        // has to look like a booted one.
        s.ensure_thread_table();
        s
    }

    fn row(tid: &str, date: i64, category: &str) -> ThreadRow {
        ThreadRow {
            thread_id: tid.into(),
            subject: format!("subject of {tid}"),
            senders_csv: "x@y.z".into(),
            count: 1,
            unread_count: 0,
            latest_date: date,
            latest_preview: "".into(),
            category: category.into(),
            importance_level: "normal".into(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred: false,
        }
    }

    #[test]
    fn lists_in_reverse_activity_order() {
        let s = store();
        let u = "u@x.com";
        // out-of-order insertion
        s.upsert_thread(u, &row("t2", 200, "inbox")).unwrap();
        s.upsert_thread(u, &row("t1", 100, "inbox")).unwrap();
        s.upsert_thread(u, &row("t3", 300, "inbox")).unwrap();
        let (got, total) = s
            .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 10)
            .unwrap();
        assert_eq!(total, 3);
        let tids: Vec<&str> = got.iter().map(|r| r.thread_id.as_str()).collect();
        assert_eq!(tids, vec!["t3", "t2", "t1"]); // highest date first
    }

    #[test]
    fn offset_and_limit_paginate() {
        let s = store();
        let u = "u@x.com";
        for i in 0..10 {
            s.upsert_thread(u, &row(&format!("t{i}"), i as i64, "inbox"))
                .unwrap();
        }
        let (got, total) = s
            .list_threads_by_activity(u, &ListThreadsFilter::default(), 3, 4)
            .unwrap();
        assert_eq!(total, 10);
        let tids: Vec<&str> = got.iter().map(|r| r.thread_id.as_str()).collect();
        // reverse activity: t9 t8 t7 [t6 t5 t4 t3] t2 t1 t0
        assert_eq!(tids, vec!["t6", "t5", "t4", "t3"]);
    }

    #[test]
    fn category_filter_uses_per_category_index() {
        let s = store();
        // the category axis is served from the declared table
        s.ensure_thread_table();
        let u = "u@x.com";
        s.upsert_thread(u, &row("a1", 100, "inbox")).unwrap();
        s.upsert_thread(u, &row("a2", 200, "social")).unwrap();
        s.upsert_thread(u, &row("a3", 300, "inbox")).unwrap();
        let f = ListThreadsFilter {
            category: Some("social"),
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].thread_id, "a2");
    }

    #[test]
    fn pinned_filter_returns_only_pinned() {
        let s = store();
        // the flag axes are served from the declared table
        s.ensure_thread_table();
        let u = "u@x.com";
        let mut p = row("p1", 100, "inbox");
        p.pinned = true;
        let np = row("p2", 200, "inbox");
        s.upsert_thread(u, &p).unwrap();
        s.upsert_thread(u, &np).unwrap();
        let f = ListThreadsFilter {
            pinned: true,
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(got[0].thread_id, "p1");
    }

    #[test]
    fn cursor_paginates_by_date() {
        let s = store();
        let u = "u@x.com";
        // 5 threads at dates 100, 200, 300, 400, 500
        for i in 1..=5 {
            s.upsert_thread(u, &row(&format!("t{i}"), i * 100, "inbox"))
                .unwrap();
        }
        // First page — no cursor, limit 2.
        let (page1, _total) = s
            .list_threads_by_activity(u, &ListThreadsFilter::default(), 0, 2)
            .unwrap();
        assert_eq!(
            page1
                .iter()
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["t5", "t4"]
        );

        // Second page — cursor = last item's latest_date = 400. Should
        // return threads STRICTLY less than 400: t3 (300), t2 (200).
        let f = ListThreadsFilter {
            before_ts: Some(400),
            ..Default::default()
        };
        let (page2, _total) = s.list_threads_by_activity(u, &f, 0, 2).unwrap();
        assert_eq!(
            page2
                .iter()
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["t3", "t2"]
        );
    }

    #[test]
    fn cursor_skips_ts_boundary() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("boundary", 500, "inbox")).unwrap();
        s.upsert_thread(u, &row("under", 499, "inbox")).unwrap();
        let f = ListThreadsFilter {
            before_ts: Some(500),
            ..Default::default()
        };
        let (rows, _total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].thread_id, "under");
    }

    #[test]
    fn folder_sent_returns_only_sent_threads() {
        let s = store();
        let u = "u@x.com";
        // Sent membership is decided by senders_csv containing the user.
        let mut sent = row("s1", 200, "inbox");
        sent.senders_csv = "me <u@x.com>".into();
        let received = row("r1", 300, "inbox");
        s.upsert_thread(u, &sent).unwrap();
        s.upsert_thread(u, &received).unwrap();
        let f = ListThreadsFilter {
            folder: Some("Sent"),
            ..Default::default()
        };
        let (rows, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].thread_id, "s1");

        // Case-insensitive match.
        let f2 = ListThreadsFilter {
            folder: Some("sent"),
            ..Default::default()
        };
        let (rows2, _) = s.list_threads_by_activity(u, &f2, 0, 10).unwrap();
        assert_eq!(rows2.len(), 1);
    }

    #[test]
    fn offset_past_end_returns_empty() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("only", 1, "inbox")).unwrap();
        let (got, total) = s
            .list_threads_by_activity(u, &ListThreadsFilter::default(), 5, 10)
            .unwrap();
        assert_eq!(total, 1);
        assert!(got.is_empty());
    }
}

/// Whether the Junk axis is served from the declared table.
///
/// Read per call rather than cached: the revert must take effect on a
/// container restart with an env change, not on a redeploy.
fn bucket_reads_table() -> bool {
    std::env::var("MAILRS_JUNK_READ").as_deref() != Ok("zset")
}

impl ListThreadsFilter<'_> {
    /// The Junk axis with nothing stacked on it.
    ///
    /// The ORDERPATH answers `(user, bucket, activity DESC)`; any extra
    /// predicate would need an intersection this path does not do, so
    /// those keep going through the zset route until their own axis is
    /// cut over.
    /// The category axis with nothing stacked on it.
    ///
    /// Separate from `bare_bucket`: a caller passes `category` without
    /// a folder, and the vocabularies differ (`spam` here is `junk`
    /// there).
    fn bare_category(&self) -> Option<&str> {
        let cat = self.category?;
        let bare = self.folder.is_none()
            && !self.pinned
            && !self.archived
            && !self.has_unread
            && !self.has_action
            && !self.starred;
        bare.then_some(cat)
    }

    /// Exactly one boolean predicate, with no folder or category.
    ///
    /// Two stacked flags would need an intersection this path does not
    /// do, so they keep going through the ZINTERSTORE route.
    fn bare_flag(&self) -> Option<&'static str> {
        if self.folder.is_some() || self.category.is_some() {
            return None;
        }
        let set: Vec<&'static str> = [
            ("starred", self.starred),
            ("archived", self.archived),
            ("pinned", self.pinned),
            ("unread", self.has_unread),
            ("has_action", self.has_action),
        ]
        .iter()
        .filter(|(_, on)| *on)
        .map(|(n, _)| *n)
        .collect();
        match set.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }

    /// The Sent axis with nothing stacked on it.
    fn is_bare_sent(&self) -> bool {
        self.folder.is_some_and(|f| f.eq_ignore_ascii_case("sent")) && self.no_predicates()
    }

    /// The merged Notifications + Promotions view.
    fn is_bare_np(&self) -> bool {
        self.folder.is_some_and(|f| f.eq_ignore_ascii_case("np")) && self.no_predicates()
    }

    /// No folder, no category, no flag — the default recency axis.
    fn is_bare_default(&self) -> bool {
        self.folder.is_none() && self.category.is_none() && self.no_predicates()
    }

    fn no_predicates(&self) -> bool {
        self.category.is_none()
            && !self.pinned
            && !self.archived
            && !self.has_unread
            && !self.has_action
            && !self.starred
    }

    /// One flag plus one scope (a folder's bucket, or a category).
    ///
    /// Returns the flag's index name and the `(column, value)` the
    /// engine should filter on. `None` when the shape is not one flag
    /// and one scope — those are handled by the bare paths, or by
    /// nothing yet.
    fn stacked_predicate(&self) -> Option<(&'static str, (&'static str, String))> {
        let flags: Vec<&'static str> = [
            ("starred", self.starred),
            ("archived", self.archived),
            ("pinned", self.pinned),
            ("unread", self.has_unread),
            ("has_action", self.has_action),
        ]
        .iter()
        .filter(|(_, on)| *on)
        .map(|(n, _)| *n)
        .collect();
        let [flag] = flags.as_slice() else {
            return None;
        };

        // A folder scopes by bucket; a category scopes by category.
        // Both cannot apply at once here.
        match (self.folder, self.category) {
            (Some(f), None) => {
                let bucket = match f {
                    f if f.eq_ignore_ascii_case("inbox") => "inbox",
                    f if f.eq_ignore_ascii_case("junk") => "junk",
                    f if f.eq_ignore_ascii_case("notifications") => "notifications",
                    f if f.eq_ignore_ascii_case("promotions") => "promotions",
                    _ => return None,
                };
                Some((flag, ("bucket", bucket.to_string())))
            }
            (None, Some(cat)) => Some((flag, ("category", cat.to_string()))),
            _ => None,
        }
    }

    fn bare_bucket(&self) -> Option<&'static str> {
        let bucket = match self.folder? {
            f if f.eq_ignore_ascii_case("junk") => "junk",
            f if f.eq_ignore_ascii_case("inbox") => "inbox",
            f if f.eq_ignore_ascii_case("notifications") => "notifications",
            f if f.eq_ignore_ascii_case("promotions") => "promotions",
            // "np" is the union of the two and needs a different shape;
            // it keeps going through the zset route.
            _ => return None,
        };
        let bare = self.category.is_none()
            && !self.pinned
            && !self.archived
            && !self.has_unread
            && !self.has_action
            && !self.starred;
        bare.then_some(bucket)
    }
}

impl KevyMailboxStore {
    /// The default axis, off the pure-recency ORDERPATH.
    fn list_default_via_table(
        &self,
        user: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let total = self.count_thread_ids_by_activity_via_table(user)?;
        if limit == 0 {
            return Ok((Vec::new(), total));
        }
        let tids = match filter.before_ts {
            Some(ts) => self.list_thread_ids_by_activity_via_table(user, limit, Some(ts - 1))?,
            None => {
                if offset >= total {
                    return Ok((Vec::new(), total));
                }
                let mut page =
                    self.list_thread_ids_by_activity_via_table(user, offset + limit, None)?;
                page.drain(..offset.min(page.len()));
                page
            }
        };
        self.hydrate_page(&tids, total)
    }

    /// The merged Notifications + Promotions view.
    ///
    /// An ORDERPATH answers one contiguous range, and this is two, so
    /// the merge happens here: take a full page from each side, then
    /// interleave by recency. Both sides are already sorted, so this
    /// is a two-way merge rather than a sort — and taking `offset +
    /// limit` from each guarantees the merged prefix is correct no
    /// matter how the two interleave.
    fn list_np_via_table(
        &self,
        user: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let total = self.count_thread_ids_by_bucket_via_table(user, "notifications")?
            + self.count_thread_ids_by_bucket_via_table(user, "promotions")?;
        if limit == 0 {
            return Ok((Vec::new(), total));
        }
        let want = offset + limit;
        let mut merged: Vec<ThreadRow> = Vec::new();
        for bucket in ["notifications", "promotions"] {
            let tids = match filter.before_ts {
                Some(ts) => {
                    self.list_thread_ids_by_bucket_before_via_table(user, bucket, ts - 1, want)?
                }
                None => self.list_thread_ids_by_bucket_via_table(user, bucket, want)?,
            };
            let (rows, _) = self.hydrate_page(&tids, 0)?;
            merged.extend(rows);
        }
        merged.sort_by(|a, b| {
            b.latest_date
                .cmp(&a.latest_date)
                .then_with(|| a.thread_id.cmp(&b.thread_id))
        });
        if offset >= merged.len() {
            return Ok((Vec::new(), total));
        }
        merged.drain(..offset);
        merged.truncate(limit);
        Ok((merged, total))
    }

    /// Serve a flag page scoped to a folder or category.
    fn list_stacked_via_table(
        &self,
        user: &str,
        flag: &str,
        scope: (&str, String),
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let extra = [(scope.0, scope.1.as_str())];
        let total = self.count_thread_ids_by_flag_filtered(user, flag, &extra)?;
        if limit == 0 {
            return Ok((Vec::new(), total));
        }
        let tids = match filter.before_ts {
            Some(ts) => {
                self.list_thread_ids_by_flag_filtered(user, flag, &extra, limit, 0, Some(ts - 1))?
            }
            None => {
                if offset >= total {
                    return Ok((Vec::new(), total));
                }
                self.list_thread_ids_by_flag_filtered(user, flag, &extra, limit, offset, None)?
            }
        };
        self.hydrate_page(&tids, total)
    }

    /// Serve one boolean-predicate page off that flag's index.
    fn list_flag_via_table(
        &self,
        user: &str,
        flag: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let total = self.count_thread_ids_by_flag_via_table(user, flag)?;
        if limit == 0 {
            return Ok((Vec::new(), total));
        }
        let tids = match filter.before_ts {
            Some(ts) => {
                self.list_thread_ids_by_flag_via_table(user, flag, limit, 0, Some(ts - 1))?
            }
            None => {
                if offset >= total {
                    return Ok((Vec::new(), total));
                }
                self.list_thread_ids_by_flag_via_table(user, flag, limit, offset, None)?
            }
        };
        self.hydrate_page(&tids, total)
    }

    /// Serve a category page off the second declared ORDERPATH.
    fn list_category_via_table(
        &self,
        user: &str,
        cat: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let total = self.count_thread_ids_by_category_via_table(user, cat)?;
        if limit == 0 {
            return Ok((Vec::new(), total));
        }
        let tids = match filter.before_ts {
            Some(ts) => {
                self.list_thread_ids_by_category_before_via_table(user, cat, ts - 1, limit)?
            }
            None => {
                if offset >= total {
                    return Ok((Vec::new(), total));
                }
                let mut page =
                    self.list_thread_ids_by_category_via_table(user, cat, offset + limit)?;
                page.drain(..offset.min(page.len()));
                page
            }
        };
        self.hydrate_page(&tids, total)
    }

    /// Serve the Junk page off the declared ORDERPATH.
    ///
    /// `before_ts` becomes a range on the `activity` component — the
    /// composite encoding puts it right after the two equality columns,
    /// which is exactly the shape composite bounds support.
    fn list_bucket_via_table(
        &self,
        user: &str,
        bucket: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        // Inbox excludes threads the user only ever sent — the zset it
        // replaces did, and a thread you sent belongs in Sent. That
        // exclusion lives in a separate ORDERPATH rather than in a
        // post-filter, so the count stays an index count.
        let unsent_only = bucket == "inbox";
        let total = if unsent_only {
            self.count_thread_ids_by_bucket_unsent_via_table(user, bucket)?
        } else {
            self.count_thread_ids_by_bucket_via_table(user, bucket)?
        };
        if limit == 0 {
            return Ok((Vec::new(), total));
        }

        let tids = match filter.before_ts {
            Some(ts) if unsent_only => {
                self.list_thread_ids_by_bucket_unsent_before_via_table(user, bucket, ts - 1, limit)?
            }
            Some(ts) => {
                self.list_thread_ids_by_bucket_before_via_table(user, bucket, ts - 1, limit)?
            }
            None => {
                if offset >= total {
                    return Ok((Vec::new(), total));
                }
                let mut page = if unsent_only {
                    self.list_thread_ids_by_bucket_unsent_via_table(user, bucket, offset + limit)?
                } else {
                    self.list_thread_ids_by_bucket_via_table(user, bucket, offset + limit)?
                };
                page.drain(..offset.min(page.len()));
                page
            }
        };

        self.hydrate_page(&tids, total)
    }

    /// Read the page's thread hashes under one shard lock, the same
    /// way the zset path does — no interleaving writer may shift a
    /// row's counters between two hgetalls within one page.
    fn hydrate_page(&self, tids: &[String], total: usize) -> io::Result<(Vec<ThreadRow>, usize)> {
        self.store()
            .atomic(|ctx| {
                let mut out = Vec::with_capacity(tids.len());
                for tid in tids {
                    let pairs = ctx.hgetall(keys::thread(tid).as_bytes())?;
                    if let Some(row) = ThreadRow::from_pairs(tid.clone(), &pairs) {
                        out.push(row);
                    }
                }
                Ok((out, total))
            })
            .map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod junk_cutover_tests {
    use super::*;
    use crate::thread_row::ThreadRow;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn row(tid: &str, activity: i64, category: &str) -> ThreadRow {
        ThreadRow {
            thread_id: tid.into(),
            subject: "s".into(),
            senders_csv: "a@x.com".into(),
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
        }
    }

    fn seeded() -> KevyMailboxStore {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();
        for (tid, when) in [("j1", 100), ("j2", 200), ("j3", 300), ("j4", 400)] {
            st.upsert_thread("alice@x.com", &row(tid, when, "spam"))
                .unwrap();
        }
        st.upsert_thread("alice@x.com", &row("keep", 500, "inbox"))
            .unwrap();
        st
    }

    fn junk_filter<'a>() -> ListThreadsFilter<'a> {
        ListThreadsFilter {
            folder: Some("Junk"),
            ..Default::default()
        }
    }

    /// The served page must be the same threads in the same order the
    /// zset path produced, including the total used for paging.
    #[test]
    fn junk_page_is_newest_first_and_excludes_other_buckets() {
        let st = seeded();
        let (rows, total) = st
            .list_threads_by_activity("alice@x.com", &junk_filter(), 0, 10)
            .unwrap();
        assert_eq!(total, 4);
        let ids: Vec<&str> = rows.iter().map(|r| r.thread_id.as_str()).collect();
        assert_eq!(ids, vec!["j4", "j3", "j2", "j1"]);
    }

    /// The exclusion the zset encoded by omission: a thread the user
    /// only ever sent belongs in Sent, not in their inbox. On prod
    /// this was 72 threads for one account.
    #[test]
    fn inbox_excludes_sent_only_threads() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        // Received: alice is not among the senders.
        st.upsert_thread("alice@x.com", &row("received", 100, "inbox"))
            .unwrap();
        // Sent-only: alice is the sole sender.
        let mut mine = row("mine", 200, "inbox");
        mine.senders_csv = "alice@x.com".into();
        mine.sent_count = mine.count;
        st.upsert_thread("alice@x.com", &mine).unwrap();

        let filter = ListThreadsFilter {
            folder: Some("Inbox"),
            ..Default::default()
        };
        let (rows, total) = st
            .list_threads_by_activity("alice@x.com", &filter, 0, 10)
            .unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.thread_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["received"],
            "a sent-only thread must not reach the inbox"
        );
        assert_eq!(total, 1, "the count must exclude it too, not just the page");
    }

    /// The case that "has ever sent" got wrong: a conversation the
    /// user took part in is still theirs to read. Reading the flag as
    /// "is a sender" dropped 190 of one account's inbox threads.
    #[test]
    fn inbox_keeps_threads_the_user_replied_in() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        let mut replied = row("replied", 100, "inbox");
        replied.senders_csv = "bob@y.com,alice@x.com".into();
        replied.count = 3;
        replied.sent_count = 1;
        st.upsert_thread("alice@x.com", &replied).unwrap();

        let filter = ListThreadsFilter {
            folder: Some("Inbox"),
            ..Default::default()
        };
        let (rows, total) = st
            .list_threads_by_activity("alice@x.com", &filter, 0, 10)
            .unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["replied"],
            "a thread the user replied in must stay in the inbox"
        );
        assert_eq!(total, 1);
    }

    /// Offset paging must not repeat or skip across page boundaries.
    #[test]
    fn junk_offset_paging_is_contiguous() {
        let st = seeded();
        let (p1, _) = st
            .list_threads_by_activity("alice@x.com", &junk_filter(), 0, 2)
            .unwrap();
        let (p2, _) = st
            .list_threads_by_activity("alice@x.com", &junk_filter(), 2, 2)
            .unwrap();
        let ids: Vec<&str> = p1
            .iter()
            .chain(p2.iter())
            .map(|r| r.thread_id.as_str())
            .collect();
        assert_eq!(ids, vec!["j4", "j3", "j2", "j1"]);
    }

    /// "Load more" passes the tail's timestamp; the next page must be
    /// strictly older, which is the range the composite answers.
    #[test]
    fn junk_cursor_returns_strictly_older_threads() {
        let st = seeded();
        let filter = ListThreadsFilter {
            before_ts: Some(300),
            ..junk_filter()
        };
        let (rows, _) = st
            .list_threads_by_activity("alice@x.com", &filter, 0, 10)
            .unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.thread_id.as_str()).collect();
        assert_eq!(ids, vec!["j2", "j1"]);
    }
}

#[cfg(test)]
mod bucket_axis_tests {
    use super::*;
    use crate::thread_row::ThreadRow;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn row(tid: &str, activity: i64, category: &str) -> ThreadRow {
        ThreadRow {
            thread_id: tid.into(),
            subject: "s".into(),
            senders_csv: "a@x.com".into(),
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
        }
    }

    /// Every mutation that touches a thread must leave the membership
    /// row agreeing with it.
    ///
    /// The row is a second copy of facts the thread hash already
    /// holds, and only `upsert_thread` used to maintain it — so any
    /// path that wrote the hash directly (move_category, mark_seen,
    /// the flag setters) silently desynchronised the axes that read
    /// from the row. This walks each mutation and re-reads the row.
    #[test]
    fn every_mutation_keeps_the_membership_row_in_step() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();
        let u = "alice@x.com";
        let mut seed = row("t", 100, "inbox");
        seed.unread_count = 2;
        st.upsert_thread(u, &seed).unwrap();

        let field = |name: &str| -> String {
            let key = crate::keys::thread_user(u, "t");
            st.store()
                .hgetall(key.as_bytes())
                .unwrap()
                .into_iter()
                .find(|(f, _)| f == name.as_bytes())
                .map(|(_, v)| String::from_utf8_lossy(&v).into_owned())
                .unwrap_or_default()
        };

        st.set_starred(u, "t", true).unwrap();
        assert_eq!(field("starred"), "1", "set_starred must update the row");

        st.set_archived(u, "t", true).unwrap();
        assert_eq!(field("archived"), "1", "set_archived must update the row");

        st.set_pinned(u, "t", true).unwrap();
        assert_eq!(field("pinned"), "1", "set_pinned must update the row");

        st.mark_seen(u, "t").unwrap();
        assert_eq!(field("unread"), "0", "mark_seen must update the row");

        st.move_category(u, "t", "spam").unwrap();
        assert_eq!(
            field("category"),
            "spam",
            "move_category must update the row"
        );
        assert_eq!(field("bucket"), "junk", "and the bucket derived from it");
    }

    /// Reclassification must remove the thread from its old category.
    ///
    /// The zset this replaces never did: on prod one account's
    /// `by_category:inbox` held 28598 entries against 6787 live rows,
    /// because nothing deleted the old entry when a thread moved.
    #[test]
    fn reclassifying_leaves_the_old_category() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        st.upsert_thread("alice@x.com", &row("t", 100, "inbox"))
            .unwrap();
        st.upsert_thread("alice@x.com", &row("t", 100, "spam"))
            .unwrap();

        let count = |cat: &str| {
            let f = ListThreadsFilter {
                category: Some(cat),
                ..Default::default()
            };
            st.list_threads_by_activity("alice@x.com", &f, 0, 10)
                .unwrap()
        };
        let (inbox, inbox_total) = count("inbox");
        assert!(
            inbox.is_empty(),
            "the old category must not keep the thread"
        );
        assert_eq!(inbox_total, 0, "nor keep counting it");
        let (spam, spam_total) = count("spam");
        assert_eq!(spam.len(), 1, "the new category must hold it");
        assert_eq!(spam_total, 1);
    }

    /// A flag stacked on a folder — the shape the UI produces every
    /// time someone opens Archived, or filters Inbox by unread.
    ///
    /// This class returned **nothing** for a day after the legacy
    /// zsets were deleted: the bare paths did not match it and the
    /// fallback was a ZINTERSTORE over indexes that no longer existed.
    #[test]
    fn a_flag_stacked_on_a_folder_is_served() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();
        let u = "alice@x.com";

        let mut archived_inbox = row("ai", 300, "inbox");
        archived_inbox.archived = true;
        st.upsert_thread(u, &archived_inbox).unwrap();

        let mut archived_junk = row("aj", 200, "spam");
        archived_junk.archived = true;
        st.upsert_thread(u, &archived_junk).unwrap();

        // Live inbox thread, not archived.
        st.upsert_thread(u, &row("live", 100, "inbox")).unwrap();

        let archived_in = |folder: &'static str| {
            let f = ListThreadsFilter {
                folder: Some(folder),
                archived: true,
                ..Default::default()
            };
            st.list_threads_by_activity(u, &f, 0, 50).unwrap()
        };

        let (rows, total) = archived_in("Inbox");
        assert_eq!(
            rows.iter()
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["ai"],
            "archived-within-Inbox must return the archived inbox thread"
        );
        assert_eq!(total, 1, "and count it");

        let (rows, _) = archived_in("Junk");
        assert_eq!(
            rows.iter()
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["aj"],
            "the scope must actually scope"
        );
    }

    /// Unread and starred stack the same way.
    #[test]
    fn unread_and_starred_stack_on_a_folder_too() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();
        let u = "alice@x.com";

        let mut unread = row("u1", 300, "inbox");
        unread.unread_count = 2;
        st.upsert_thread(u, &unread).unwrap();

        let mut starred = row("s1", 200, "inbox");
        starred.starred = true;
        st.upsert_thread(u, &starred).unwrap();

        st.upsert_thread(u, &row("plain", 100, "inbox")).unwrap();

        let ids = |f: ListThreadsFilter<'_>| {
            st.list_threads_by_activity(u, &f, 0, 50)
                .unwrap()
                .0
                .into_iter()
                .map(|r| r.thread_id)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            ids(ListThreadsFilter {
                folder: Some("Inbox"),
                has_unread: true,
                ..Default::default()
            }),
            vec!["u1".to_string()]
        );
        assert_eq!(
            ids(ListThreadsFilter {
                folder: Some("Inbox"),
                starred: true,
                ..Default::default()
            }),
            vec!["s1".to_string()]
        );
    }

    /// The np view is the only axis whose order this code produces
    /// rather than the engine — a two-way merge of two sorted sides.
    /// Interleave the two buckets so a merge bug cannot hide behind
    /// one side happening to be newer throughout.
    #[test]
    fn np_merges_both_buckets_by_recency() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();
        let u = "alice@x.com";
        for (tid, when, cat) in [
            ("n1", 100, "notification"),
            ("p1", 150, "promotion"),
            ("n2", 200, "notification"),
            ("p2", 250, "promotion"),
        ] {
            st.upsert_thread(u, &row(tid, when, cat)).unwrap();
        }
        // Must not appear: neither bucket.
        st.upsert_thread(u, &row("inb", 999, "inbox")).unwrap();

        let f = ListThreadsFilter {
            folder: Some("np"),
            ..Default::default()
        };
        let (rows, total) = st.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["p2", "n2", "p1", "n1"],
            "the two buckets must interleave by recency"
        );
        assert_eq!(total, 4);

        // And paging through the merge must stay contiguous.
        let (p1, _) = st.list_threads_by_activity(u, &f, 0, 2).unwrap();
        let (p2, _) = st.list_threads_by_activity(u, &f, 2, 2).unwrap();
        assert_eq!(
            p1.iter()
                .chain(p2.iter())
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["p2", "n2", "p1", "n1"],
            "offset paging across a merge must not repeat or skip"
        );
    }

    /// Sent is the sent_only flag, and it is the complement of what
    /// the inbox axis excludes.
    #[test]
    fn sent_axis_holds_only_sent_only_threads() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();
        let u = "alice@x.com";

        let mut mine = row("mine", 200, "inbox");
        mine.senders_csv = u.into();
        mine.sent_count = mine.count;
        st.upsert_thread(u, &mine).unwrap();

        let mut replied = row("replied", 100, "inbox");
        replied.senders_csv = format!("bob@y.com,{u}");
        replied.count = 3;
        replied.sent_count = 1;
        st.upsert_thread(u, &replied).unwrap();

        // Never written in — must not be in Sent.
        st.upsert_thread(u, &row("theirs", 50, "inbox")).unwrap();

        let f = ListThreadsFilter {
            folder: Some("Sent"),
            ..Default::default()
        };
        let (rows, total) = st.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| r.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["mine", "replied"],
            "Sent holds every thread the user wrote in, replies included"
        );
        assert_eq!(total, 2);
    }

    /// Each bucket is a partition: a thread lands in exactly one of
    /// them, so every axis must exclude the other three.
    #[test]
    fn bucket_axes_partition_the_threads() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        for (tid, cat) in [
            ("i", "inbox"),
            ("n", "notification"),
            ("p", "promotion"),
            ("j", "spam"),
        ] {
            st.upsert_thread("alice@x.com", &row(tid, 100, cat))
                .unwrap();
        }

        for (folder, want) in [
            ("Inbox", "i"),
            ("Notifications", "n"),
            ("Promotions", "p"),
            ("Junk", "j"),
        ] {
            let filter = ListThreadsFilter {
                folder: Some(folder),
                ..Default::default()
            };
            let (rows, total) = st
                .list_threads_by_activity("alice@x.com", &filter, 0, 10)
                .unwrap();
            assert_eq!(
                rows.iter()
                    .map(|r| r.thread_id.as_str())
                    .collect::<Vec<_>>(),
                vec![want],
                "{folder} must hold exactly its own bucket"
            );
            assert_eq!(total, 1, "{folder} count must exclude the other buckets");
        }
    }
}
