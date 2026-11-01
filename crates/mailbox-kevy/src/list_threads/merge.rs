//! Lists that are two index reads of the same question, merged by
//! recency: held mail beside the live list, and All Mail's two sides of
//! the archive.

use std::io;

use crate::KevyMailboxStore;
use crate::thread_row::ThreadRow;

use super::{ListThreadsFilter, QuarantineScope};

impl KevyMailboxStore {
    /// The live page and the held one, merged by recency.
    ///
    /// Two index reads and two index counts, with the total their sum.
    /// The alternative — fetching the live page and pasting held rows
    /// onto it — would be filtering a page that had already been
    /// counted, which is the exact defect the `archived` axis was
    /// fixed for on 2026-08-05.
    ///
    /// Taking `offset + limit` from each side guarantees the merged
    /// prefix is right however the two interleave; the same argument
    /// `list_buckets_via_table` makes.
    pub(super) fn merge_held_into_live(
        &self,
        user: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let mut merged: Vec<ThreadRow> = Vec::new();
        let mut total = 0usize;
        for pass in [QuarantineScope::Hide, QuarantineScope::Only] {
            let one = ListThreadsFilter {
                quarantine: pass,
                ..filter.clone()
            };
            let (rows, n) = self.list_threads_by_activity(user, &one, 0, offset + limit)?;
            total += n;
            merged.extend(rows);
        }
        Ok(page_of(merged, total, offset, limit))
    }

    /// All Mail: the live side and the archived side of the same
    /// question, each an index range with its own count, merged by
    /// recency. Archived rows come off the `archived` flag index with
    /// the scope stacked on as value filters — the shape the Archived
    /// list already reads.
    pub(super) fn merge_archived_into_live(
        &self,
        user: &str,
        filter: &ListThreadsFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> io::Result<(Vec<ThreadRow>, usize)> {
        let mut merged: Vec<ThreadRow> = Vec::new();
        let mut total = 0usize;
        for archived in [false, true] {
            let one = ListThreadsFilter {
                archived,
                ..filter.clone()
            };
            let (rows, n) = self.list_one_side(user, &one, 0, offset + limit)?;
            total += n;
            merged.extend(rows);
        }
        Ok(page_of(merged, total, offset, limit))
    }
}

/// Recency order, then one page of it.
pub(super) fn page_of(
    mut merged: Vec<ThreadRow>,
    total: usize,
    offset: usize,
    limit: usize,
) -> (Vec<ThreadRow>, usize) {
    merged.sort_by(|a, b| {
        b.latest_date
            .cmp(&a.latest_date)
            .then_with(|| a.thread_id.cmp(&b.thread_id))
    });
    if offset >= merged.len() {
        return (Vec::new(), total);
    }
    merged.drain(..offset);
    merged.truncate(limit);
    (merged, total)
}
