//! One function per axis the conversation list can ask for.
//!
//! Each takes a page of thread ids off a declared index and hydrates it
//! from the caller's own membership rows — never from the shared thread
//! hash, which has no user segment and would serve one owner's counters
//! to the other.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;
use crate::thread_row::ThreadRow;

use super::{FlagKey, ListThreadsFilter};
use crate::table_query::ArchiveScope;

/// Every function here serves a list the user navigated to, and
/// Archived is not one of them — it has a flag on, so it leaves the
/// dispatcher through the flag branch and never arrives here.
const LIVE: ArchiveScope = ArchiveScope::Live;

impl KevyMailboxStore {
    /// The default axis, off the pure-recency ORDERPATH.
    pub(super) fn list_default_via_table(
        &self,
        user: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let total = self.count_thread_ids_by_activity_via_table(user, LIVE)?;
        if limit == 0 {
            return Ok((Vec::new(), total));
        }
        let tids = match filter.before_ts {
            Some(ts) => {
                self.list_thread_ids_by_activity_via_table(user, LIVE, limit, Some(ts - 1))?
            }
            None => {
                if offset >= total {
                    return Ok((Vec::new(), total));
                }
                let mut page =
                    self.list_thread_ids_by_activity_via_table(user, LIVE, offset + limit, None)?;
                page.drain(..offset.min(page.len()));
                page
            }
        };
        self.hydrate_page(user, &tids, total)
    }

    /// Several buckets merged into one recency-ordered view.
    ///
    /// An ORDERPATH answers one contiguous range, and this is N, so the
    /// merge happens here: take a full page from each side, then
    /// interleave by recency. Each side is already sorted, so this is a
    /// merge rather than a sort — and taking `offset + limit` from each
    /// guarantees the merged prefix is correct no matter how they
    /// interleave.
    ///
    /// The `inbox` bucket reads off the sent-aware axis, the same one
    /// [`Self::list_bucket_via_table`] uses for it: a thread the user
    /// only ever sent into is not in their Inbox, and must not arrive
    /// here by the side door.
    pub(super) fn list_buckets_via_table(
        &self,
        user: &str,
        buckets: &[&str],
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let mut total = 0usize;
        for b in buckets {
            total += if *b == "inbox" {
                self.count_thread_ids_by_bucket_unsent_via_table(user, b, LIVE)?
            } else {
                self.count_thread_ids_by_bucket_via_table(user, b, LIVE)?
            };
        }
        if limit == 0 {
            return Ok((Vec::new(), total));
        }
        let want = offset + limit;
        let mut merged: Vec<ThreadRow> = Vec::new();
        for bucket in buckets.iter().copied() {
            let tids = match (bucket, filter.before_ts) {
                ("inbox", Some(ts)) => self.list_thread_ids_by_bucket_unsent_before_via_table(
                    user,
                    bucket,
                    LIVE,
                    ts - 1,
                    want,
                )?,
                ("inbox", None) => {
                    self.list_thread_ids_by_bucket_unsent_via_table(user, bucket, LIVE, want)?
                }
                (_, Some(ts)) => self.list_thread_ids_by_bucket_before_via_table(
                    user,
                    bucket,
                    LIVE,
                    ts - 1,
                    want,
                )?,
                (_, None) => self.list_thread_ids_by_bucket_via_table(user, bucket, LIVE, want)?,
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

    /// The same merge with flags stacked on it.
    ///
    /// Still N ranges, so still a merge — but keyed on the flag index
    /// rather than the bucket ORDERPATH, because that is the one that
    /// can carry the other predicates as value filters. The bucket
    /// becomes one of those filters, and for `inbox` so does the
    /// sent-only exclusion.
    pub(super) fn list_buckets_flagged_via_table(
        &self,
        user: &str,
        buckets: &[&str],
        key: FlagKey<'_>,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let FlagKey { flag, extra } = key;
        let want = offset + limit;
        let mut total = 0usize;
        let mut merged: Vec<ThreadRow> = Vec::new();
        for bucket in buckets.iter().copied() {
            let scoped = super::bucket_predicates(bucket, extra);
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
    pub(super) fn list_stacked_via_table(
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
    pub(super) fn list_flag_via_table(
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
    pub(super) fn list_category_via_table(
        &self,
        user: &str,
        cat: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let total = self.count_thread_ids_by_category_via_table(user, cat, LIVE)?;
        if limit == 0 {
            return Ok((Vec::new(), total));
        }
        let tids = match filter.before_ts {
            Some(ts) => {
                self.list_thread_ids_by_category_before_via_table(user, cat, LIVE, ts - 1, limit)?
            }
            None => {
                if offset >= total {
                    return Ok((Vec::new(), total));
                }
                let mut page =
                    self.list_thread_ids_by_category_via_table(user, cat, LIVE, offset + limit)?;
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
    pub(super) fn list_bucket_via_table(
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
            self.count_thread_ids_by_bucket_unsent_via_table(user, bucket, LIVE)?
        } else {
            self.count_thread_ids_by_bucket_via_table(user, bucket, LIVE)?
        };
        if limit == 0 {
            return Ok((Vec::new(), total));
        }

        let tids = match filter.before_ts {
            Some(ts) if unsent_only => self.list_thread_ids_by_bucket_unsent_before_via_table(
                user,
                bucket,
                LIVE,
                ts - 1,
                limit,
            )?,
            Some(ts) => {
                self.list_thread_ids_by_bucket_before_via_table(user, bucket, LIVE, ts - 1, limit)?
            }
            None => {
                if offset >= total {
                    return Ok((Vec::new(), total));
                }
                let mut page = if unsent_only {
                    self.list_thread_ids_by_bucket_unsent_via_table(
                        user,
                        bucket,
                        LIVE,
                        offset + limit,
                    )?
                } else {
                    self.list_thread_ids_by_bucket_via_table(user, bucket, LIVE, offset + limit)?
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
    pub(super) fn hydrate_page(
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
            .map_err(std::io::Error::from)
    }
}
