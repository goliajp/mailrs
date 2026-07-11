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
        // v2 Stage B.4/B.6: kevy 3.17 ships ZINTERSTORE — when the
        // caller stacks ≥ 2 predicates (e.g. inbox ∩ has_unread),
        // materialize the intersection into a per-request temp zset
        // scored by the max latest_date. Prior implementation walked
        // the highest-priority single index and let the UI show
        // over-count badges. The temp key is TTL-tagged so an orphan
        // (e.g. panic mid-request) auto-cleans.
        let index_keys = filter.predicate_index_keys(user);
        let owned_temp: Option<String> = if index_keys.len() > 1 {
            let ts_nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let temp = format!("mailrs:tmp:zinter:{user}:{ts_nanos}");
            let refs: Vec<&[u8]> = index_keys.iter().map(|k| k.as_bytes()).collect();
            self.store().zinterstore(
                temp.as_bytes(),
                &refs,
                None,
                kevy_embedded::ZAggregate::Max,
            )?;
            self.store()
                .expire(temp.as_bytes(), std::time::Duration::from_secs(60))?;
            Some(temp)
        } else {
            None
        };
        let key: &str = owned_temp
            .as_deref()
            .unwrap_or_else(|| index_keys[0].as_str());
        let total = self.store().zcard(key.as_bytes())?;
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
                .zrev_range_by_score(key.as_bytes(), max, f64::NEG_INFINITY)?;
            raw.into_iter().take(limit).collect()
        } else {
            if offset >= total {
                return Ok((Vec::new(), total));
            }
            let stop_exclusive = offset + limit;
            let stop_inclusive_idx = (stop_exclusive.min(total) as i64) - 1;
            self.store()
                .zrevrange(key.as_bytes(), offset as i64, stop_inclusive_idx)?
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
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));
        KevyMailboxStore::new(s)
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
