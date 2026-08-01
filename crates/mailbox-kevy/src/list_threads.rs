//! `list_threads_by_activity` — every conversation-list page.
//!
//! One dispatcher over the declared `threaduser` table. A filter names a
//! scope (a folder, a category, or neither) and any number of boolean
//! flags; this picks the axis to key on and hands the rest to the engine
//! as value filters.
//!
//! It is deliberately **total**. The previous shape enumerated the
//! combinations it knew — one bare flag, one flag plus one scope — and let
//! everything else fall through to the hand-maintained zsets. Once those
//! were dropped that fall-through returned an empty page: two flags at once
//! ("starred" plus "unread only"), or a folder and a category together,
//! answered `[]` with a 200 and no way to tell it from an empty mailbox.
//! Measured 2026-08-01: all fifteen legacy zsets held 0 rows across all 13
//! prod accounts, so every one of those combinations was already blank.
//!
//! The one shape that genuinely has no rows — a folder and a category that
//! disagree, say Junk ∩ `promotion` — returns empty because it *is* empty,
//! and says so in one place rather than by falling off the end.

use std::io;

use super::KevyMailboxStore;
use super::keys;
use super::thread_row::ThreadRow;

/// Filter knobs passed to `list_threads_by_activity`. None of these are
/// required; default is "all threads sorted by recency, latest first."
#[derive(Debug, Clone, Default)]
pub struct ListThreadsFilter<'a> {
    /// Restrict to a single category (`inbox`, `social`, `spam`, …) —
    /// the `category` column on the membership row.
    pub category: Option<&'a str>,
    /// Match monolith's `folder` query: `inbox`, `junk`, `notifications`,
    /// `promotions`, `np` (the merged notifications+promotions view) or
    /// `sent`, case-insensitively. Anything else is no scope at all.
    pub folder: Option<&'a str>,
    /// Only threads with `pinned = true`.
    pub pinned: bool,
    /// Only threads with `archived = true`.
    pub archived: bool,
    /// Only threads with `unread_count > 0`.
    pub has_unread: bool,
    /// Only threads with `has_action = true`.
    pub has_action: bool,
    /// Only threads with `starred = true`.
    pub starred: bool,
    /// Cursor for pagination: only return threads with `latest_date <
    /// before_ts`. `activity` is the component right after the equality
    /// columns in every declared composite, which is the one position a
    /// range may constrain — so this is the shape the ORDERPATHs were
    /// designed for. When `None`, the caller windows with
    /// `(offset, limit)`.
    pub before_ts: Option<i64>,
}

/// What the filter's `folder` / `category` pair scopes the page to,
/// after the two have been reconciled.
#[derive(Debug, Clone, PartialEq)]
enum Scope {
    /// No folder and no category — every thread the user has.
    All,
    /// One folder bucket: `inbox`, `junk`, `notifications`, `promotions`.
    Bucket(&'static str),
    /// The merged notifications+promotions view, which is two ranges.
    Np,
    /// One category, which implies its bucket and is narrower than it.
    Category(String),
    /// A folder and a category that cannot both hold — `junk` with
    /// `promotion`, say. No thread satisfies it, and that is an answer.
    Contradiction,
}

impl ListThreadsFilter<'_> {
    /// Every boolean flag the filter has switched on, as the column
    /// names the membership row stores them under, **first one first**.
    ///
    /// `sent` is a folder to the caller and a flag here: `is_sender` is
    /// a declared column like the other five, so treating it as one
    /// means "Sent, unread only" needs no special case.
    ///
    /// It is first on purpose. Each flag index stores the other columns
    /// beside it so a second predicate can be a FILTER rather than an
    /// intersection — and `is_sender` is the one column that list omits
    /// (`thread_user_spec`, asserted by `is_sender_is_the_only_flag_that_must_be_the_key`).
    /// So it can be keyed on but not filtered on, and the caller keys on
    /// whichever flag comes back first.
    fn flags_on(&self) -> Vec<&'static str> {
        let mut out = Vec::with_capacity(6);
        for (name, on) in [
            (
                "is_sender",
                self.folder.is_some_and(|f| f.eq_ignore_ascii_case("sent")),
            ),
            ("starred", self.starred),
            ("archived", self.archived),
            ("pinned", self.pinned),
            ("unread", self.has_unread),
            ("has_action", self.has_action),
        ] {
            if on {
                out.push(name);
            }
        }
        out
    }

    /// Reconcile `folder` and `category` into the one scope the page is
    /// keyed on.
    ///
    /// A category implies its bucket — `bucket_of` maps every category
    /// onto exactly one — so naming both is either redundant (and the
    /// category is the narrower of the two) or impossible. Saying which
    /// here is what keeps the dispatcher total.
    fn scope(&self) -> Scope {
        let folder_bucket = self.folder.and_then(|f| match f {
            f if f.eq_ignore_ascii_case("inbox") => Some(keys::Bucket::Inbox),
            f if f.eq_ignore_ascii_case("junk") => Some(keys::Bucket::Junk),
            f if f.eq_ignore_ascii_case("notifications") => Some(keys::Bucket::Notifications),
            f if f.eq_ignore_ascii_case("promotions") => Some(keys::Bucket::Promotions),
            _ => None,
        });
        let is_np = self.folder.is_some_and(|f| f.eq_ignore_ascii_case("np"));

        match (self.category, folder_bucket, is_np) {
            (Some(cat), Some(b), _) if keys::bucket_of(cat) != b => Scope::Contradiction,
            (Some(cat), _, true)
                if !matches!(
                    keys::bucket_of(cat),
                    keys::Bucket::Notifications | keys::Bucket::Promotions
                ) =>
            {
                Scope::Contradiction
            }
            (Some(cat), _, _) => Scope::Category(cat.to_string()),
            (None, Some(b), _) => Scope::Bucket(b.name()),
            (None, None, true) => Scope::Np,
            (None, None, false) => Scope::All,
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
        // One dispatcher, total over the filter space. Every branch ends
        // in a declared index; nothing falls through, because what it
        // used to fall through to — the hand-maintained zsets — is gone,
        // and answered every combination it covered with an empty page.
        let scope = filter.scope();
        if scope == Scope::Contradiction {
            return Ok((Vec::new(), 0));
        }
        let flags = filter.flags_on();

        // No flag set: the scope's own ORDERPATH answers it directly,
        // and its total is an index count rather than a walk.
        let Some((key_flag, rest)) = flags.split_first() else {
            return match scope {
                Scope::All => self.list_default_via_table(user, filter, offset, limit),
                Scope::Bucket(b) => self.list_bucket_via_table(user, b, filter, offset, limit),
                Scope::Np => self.list_np_via_table(user, filter, offset, limit),
                Scope::Category(cat) => {
                    self.list_category_via_table(user, &cat, filter, offset, limit)
                }
                Scope::Contradiction => unreachable!("returned above"),
            };
        };

        // At least one flag: key on the first, and hand the engine every
        // other predicate as a value filter. Each flag index stores user
        // and activity beside the flag, so any one of them can be the
        // key and the rest cost a comparison — which is why "starred and
        // unread, within Inbox" needs no index of its own.
        let mut extra: Vec<(&str, String)> = rest.iter().map(|f| (*f, "1".to_string())).collect();
        match scope {
            Scope::All => {}
            Scope::Bucket(b) => {
                extra.push(("bucket", b.to_string()));
                if b == "inbox" {
                    // Inbox excludes threads that are nothing but the
                    // user's own messages — the same exclusion the
                    // bucket ORDERPATH carries in its sort prefix.
                    extra.push(("sent_only", "0".to_string()));
                }
            }
            Scope::Category(cat) => extra.push(("category", cat)),
            Scope::Np => {
                return self
                    .list_np_flagged_via_table(user, key_flag, &extra, filter, offset, limit);
            }
            Scope::Contradiction => unreachable!("returned above"),
        }
        self.list_stacked_via_table(user, key_flag, &extra, filter, offset, limit)
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
        s.upsert_thread(u, &row("p1", 100, "inbox")).unwrap();
        s.upsert_thread(u, &row("p2", 200, "inbox")).unwrap();
        s.set_pinned(u, "p1", true).unwrap();
        let f = ListThreadsFilter {
            pinned: true,
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(got[0].thread_id, "p1");
    }

    /// Two flags at once is an intersection, not an empty page.
    ///
    /// This is the shape that had no declared path: `bare_flag` returned
    /// `None` for two, `stacked_predicate` returned `None` for two, and
    /// what caught it was the zset intersection — over zsets that hold 0
    /// rows on every prod account. The tab answered `[]` with a 200.
    #[test]
    fn two_flags_at_once_intersect() {
        let s = store();
        let u = "u@x.com";
        for (tid, when) in [("both", 300), ("star", 200), ("unread", 100)] {
            s.upsert_thread(u, &row(tid, when, "inbox")).unwrap();
        }
        // Through the mutators, which is where per-user state is set.
        for tid in ["both", "star"] {
            s.set_starred(u, tid, true).unwrap();
        }
        for tid in ["both", "unread"] {
            s.mark_unread(u, tid).unwrap();
        }

        let f = ListThreadsFilter {
            starred: true,
            has_unread: true,
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1, "starred ∩ unread is one thread, not none");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].thread_id, "both");
    }

    /// Two flags *and* a folder — the same shape with a scope on top.
    #[test]
    fn two_flags_within_a_folder_stay_inside_it() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("in", 300, "inbox")).unwrap();
        s.upsert_thread(u, &row("junk", 400, "spam")).unwrap();
        for tid in ["in", "junk"] {
            s.set_starred(u, tid, true).unwrap();
            s.mark_unread(u, tid).unwrap();
        }

        let f = ListThreadsFilter {
            folder: Some("inbox"),
            starred: true,
            has_unread: true,
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(got[0].thread_id, "in");
    }

    /// A folder and a category name the same axis twice. The category is
    /// the narrower of the two, and the page is its page — not empty,
    /// which is what having no path for the pair used to produce.
    #[test]
    fn a_folder_and_an_agreeing_category_use_the_narrower_one() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("social", 300, "social")).unwrap();
        s.upsert_thread(u, &row("plain", 200, "inbox")).unwrap();

        let f = ListThreadsFilter {
            folder: Some("inbox"),
            category: Some("social"),
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1, "`social` sits in the inbox bucket");
        assert_eq!(got[0].thread_id, "social");
    }

    /// A folder and a category that cannot both hold is genuinely empty,
    /// and says so in one place rather than by falling off the end.
    #[test]
    fn a_folder_and_a_disagreeing_category_are_empty() {
        let s = store();
        let u = "u@x.com";
        s.upsert_thread(u, &row("promo", 300, "promotion")).unwrap();
        s.upsert_thread(u, &row("spam", 200, "spam")).unwrap();

        let f = ListThreadsFilter {
            folder: Some("junk"),
            category: Some("promotion"),
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 0);
        assert!(got.is_empty());
    }

    /// The merged Notifications+Promotions view with a flag on it: two
    /// ranges, keyed on the flag, merged by recency.
    #[test]
    fn np_with_a_flag_merges_both_buckets() {
        let s = store();
        let u = "u@x.com";
        for (tid, when, cat) in [
            ("n1", 100, "notification"),
            ("p1", 300, "promotion"),
            ("p2", 400, "promotion"),
            ("i1", 500, "inbox"),
        ] {
            s.upsert_thread(u, &row(tid, when, cat)).unwrap();
        }
        for tid in ["n1", "p1", "i1"] {
            s.set_starred(u, tid, true).unwrap();
        }

        let f = ListThreadsFilter {
            folder: Some("np"),
            starred: true,
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 2, "starred within notifications ∪ promotions");
        let tids: Vec<&str> = got.iter().map(|r| r.thread_id.as_str()).collect();
        assert_eq!(tids, vec!["p1", "n1"], "merged newest-first");
    }

    /// Sent is a flag, so a predicate stacks on it like any other.
    #[test]
    fn sent_takes_a_stacked_flag() {
        let s = store();
        let u = "u@x.com";
        let mut sent_unread = row("s1", 300, "inbox");
        sent_unread.senders_csv = u.into();
        let mut sent_read = row("s2", 200, "inbox");
        sent_read.senders_csv = u.into();
        s.upsert_thread(u, &sent_unread).unwrap();
        s.upsert_thread(u, &sent_read).unwrap();
        s.mark_unread(u, "s1").unwrap();

        let f = ListThreadsFilter {
            folder: Some("sent"),
            has_unread: true,
            ..Default::default()
        };
        let (got, total) = s.list_threads_by_activity(u, &f, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(got[0].thread_id, "s1");
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
        self.hydrate_page(user, &tids, total)
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
            let (rows, _) = self.hydrate_page(user, &tids, 0)?;
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

    /// The merged Notifications + Promotions view with flags stacked on
    /// it.
    ///
    /// Still two ranges, so still a two-way merge — but keyed on the
    /// flag index rather than the bucket ORDERPATH, because that is the
    /// one that can carry the other predicates as value filters. The
    /// bucket becomes one of those filters.
    fn list_np_flagged_via_table(
        &self,
        user: &str,
        flag: &str,
        extra: &[(&str, String)],
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let want = offset + limit;
        let mut total = 0usize;
        let mut merged: Vec<ThreadRow> = Vec::new();
        for bucket in ["notifications", "promotions"] {
            let mut scoped: Vec<(&str, String)> = extra.to_vec();
            scoped.push(("bucket", bucket.to_string()));
            let refs: Vec<(&str, &str)> = scoped.iter().map(|(c, v)| (*c, v.as_str())).collect();
            total += self.count_thread_ids_by_flag_filtered(user, flag, &refs)?;
            if limit == 0 {
                continue;
            }
            let tids = match filter.before_ts {
                Some(ts) => {
                    self.list_thread_ids_by_flag_filtered(user, flag, &refs, want, 0, Some(ts - 1))?
                }
                None => self.list_thread_ids_by_flag_filtered(user, flag, &refs, want, 0, None)?,
            };
            let (rows, _) = self.hydrate_page(user, &tids, 0)?;
            merged.extend(rows);
        }
        if limit == 0 {
            return Ok((Vec::new(), total));
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

    /// Serve a page keyed on one flag index, with every other predicate
    /// applied as a value filter.
    ///
    /// `extra` carries the remaining flags and the scope, already in the
    /// membership row's own column vocabulary — the caller decides which
    /// flag is the key, this does not care how many follow it.
    fn list_stacked_via_table(
        &self,
        user: &str,
        flag: &str,
        extra: &[(&str, String)],
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let extra: Vec<(&str, &str)> = extra.iter().map(|(c, v)| (*c, v.as_str())).collect();
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
        self.hydrate_page(user, &tids, total)
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
        self.hydrate_page(user, &tids, total)
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
        self.hydrate_page(user, &tids, total)
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

        self.hydrate_page(user, &tids, total)
    }

    /// Read the page's thread hashes under one shard lock, the same
    /// way the zset path does — no interleaving writer may shift a
    /// row's counters between two hgetalls within one page.
    /// Turn a page of thread ids into rows, from **each user's own**
    /// membership row.
    ///
    /// It read the shared `mailrs:thread:{tid}` hash until 2026-08-01.
    /// That hash has no user segment, so on a thread two accounts both
    /// received — 74 of 30,586 on production — the counters, the flags,
    /// the category and the preview were whichever owner wrote last, and
    /// both of them saw it. The membership row carries the same payload
    /// per user, which is what the index already keys on, so this reads
    /// one hash per row exactly as before.
    ///
    /// A tid whose row is missing is skipped rather than filled from the
    /// shared hash: the index that produced this page is built from those
    /// rows, so a gap here means something wrote an index entry without a
    /// row, and serving somebody else's copy is how that stays invisible.
    fn hydrate_page(
        &self,
        user: &str,
        tids: &[String],
        total: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        self.store()
            .atomic(|ctx| {
                let mut out = Vec::with_capacity(tids.len());
                for tid in tids {
                    let pairs = ctx.hgetall(keys::thread_user(user, tid).as_bytes())?;
                    if let Some(row) = ThreadRow::from_user_pairs(tid.clone(), &pairs) {
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

        st.upsert_thread(u, &row("ai", 300, "inbox")).unwrap();
        st.upsert_thread(u, &row("aj", 200, "spam")).unwrap();
        // Live inbox thread, not archived.
        st.upsert_thread(u, &row("live", 100, "inbox")).unwrap();
        // Archiving is a per-user act, so it goes through the mutator.
        for tid in ["ai", "aj"] {
            st.set_archived(u, tid, true).unwrap();
        }

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

        st.upsert_thread(u, &row("u1", 300, "inbox")).unwrap();
        st.upsert_thread(u, &row("s1", 200, "inbox")).unwrap();
        st.upsert_thread(u, &row("plain", 100, "inbox")).unwrap();
        st.mark_unread(u, "u1").unwrap();
        st.set_starred(u, "s1", true).unwrap();

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
