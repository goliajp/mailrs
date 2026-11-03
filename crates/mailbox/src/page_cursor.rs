//! Paging a conversation list without losing the ones that share a second.
//!
//! A list is newest first. Paging by time alone (`last_date < before_ts`)
//! drops every conversation that shares the boundary second with the last
//! row of the page: the next page starts strictly below it. So a page is
//! ordered by its time and then by a second key every store computes the
//! same way, and the cursor a row carries names both.
//!
//! The second key is [`tid_ord`](crate::page_cursor::tid_ord), the one the Maildir core's composite
//! indexes already end on (a thread id itself can exceed what one index
//! component holds). Using it in every store also makes two stores list the
//! conversations of one second in the same order.
//!
//! [`Pager`](crate::page_cursor::Pager) is store-agnostic: it says what to fetch, a store fetches it
//! with its ordinary `before_ts` paging, and the pager keeps asking for more
//! until the boundary second is complete, so no conversation of it is cut
//! off from both pages.

/// A bounded tie-breaker derived from the thread id: FNV-1a folded into a
/// non-negative `i64`.
pub fn tid_ord(tid: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in tid.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (h >> 1) as i64
}

/// Where the previous page ended: its last row's time and second key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// The row's time: its last activity, or when it began waiting.
    pub at: i64,
    /// [`tid_ord`] of the row's thread.
    pub ord: i64,
}

impl Cursor {
    /// The cursor a row with this time and thread id carries.
    pub fn of(at: i64, tid: &str) -> Cursor {
        Cursor {
            at,
            ord: tid_ord(tid),
        }
    }

    /// `"<at>:<ord>"`, what a client hands back unread.
    pub fn encode(&self) -> String {
        format!("{}:{}", self.at, self.ord)
    }

    /// A cursor a client handed back; `None` when it is not one.
    pub fn parse(s: &str) -> Option<Cursor> {
        let (at, ord) = s.split_once(':')?;
        Some(Cursor {
            at: at.parse().ok()?,
            ord: ord.parse().ok()?,
        })
    }

    /// Whether a row at `at` with key `ord` comes after this cursor.
    fn precedes(&self, at: i64, ord: i64) -> bool {
        at < self.at || (at == self.at && ord > self.ord)
    }
}

/// Rows fetched beyond the page on the first ask, to leave room for the
/// rows of the boundary second that sit before the cursor.
const SLACK: usize = 8;
/// The most rows asked for in one round; a list that exhausts it is
/// answered as far as it got.
const MAX_FETCH: usize = 100_000;

/// One page, asked for in rounds: [`Pager::ask`] says what to fetch,
/// [`Pager::offer`] takes what came back and returns the page once the
/// boundary second is complete.
pub struct Pager {
    after: Option<Cursor>,
    limit: usize,
    fetch: usize,
}

/// What one round fetches: rows with time below `before_ts` (all when
/// `None`), at most `limit` of them, newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ask {
    /// Only rows with a time below this.
    pub before_ts: Option<i64>,
    /// At most this many rows.
    pub limit: usize,
}

impl Pager {
    /// A page of `limit` rows after `after`, or the first page.
    pub fn new(after: Option<Cursor>, limit: usize) -> Pager {
        Pager {
            after,
            limit,
            fetch: limit.saturating_add(SLACK).min(MAX_FETCH),
        }
    }

    /// What the next round fetches.
    pub fn ask(&self) -> Ask {
        Ask {
            before_ts: self.after.map(|c| c.at.saturating_add(1)),
            limit: self.fetch,
        }
    }

    /// `rows` is what [`Pager::ask`] fetched, newest first by `at`. The
    /// page, ordered by time then second key, or `None` when another
    /// round with a larger fetch is needed.
    pub fn offer<T>(
        &mut self,
        rows: Vec<T>,
        at: impl Fn(&T) -> i64,
        tid: impl Fn(&T) -> &str,
    ) -> Option<Vec<T>> {
        let exhausted = rows.len() < self.fetch || self.fetch >= MAX_FETCH;
        let lowest = rows.last().map(&at);
        let mut kept: Vec<(i64, i64, T)> = rows
            .into_iter()
            .map(|r| (at(&r), tid_ord(tid(&r)), r))
            .filter(|(a, o, _)| self.after.is_none_or(|c| c.precedes(*a, *o)))
            .collect();
        kept.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));
        // complete when every row of the boundary second was fetched: the
        // fetch reached a second below it, or there was nothing more
        let complete = match (kept.get(self.limit.saturating_sub(1)), lowest) {
            (_, _) if exhausted => true,
            (Some(boundary), Some(low)) => kept.len() >= self.limit && boundary.0 > low,
            _ => false,
        };
        if !complete {
            self.fetch = self.fetch.saturating_mul(2).min(MAX_FETCH);
            return None;
        }
        kept.truncate(self.limit);
        Some(kept.into_iter().map(|(_, _, r)| r).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store's `before_ts` paging over `rows`, newest first, ties in the
    /// order given — not in second-key order, as a merge of two lists may
    /// leave them.
    fn store<'a>(rows: &'a [(i64, &'a str)], ask: Ask) -> Vec<(i64, &'a str)> {
        let mut out: Vec<(i64, &str)> = rows
            .iter()
            .copied()
            .filter(|(a, _)| ask.before_ts.is_none_or(|b| *a < b))
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.0));
        out.truncate(ask.limit);
        out
    }

    fn page<'a>(
        rows: &'a [(i64, &'a str)],
        after: Option<Cursor>,
        limit: usize,
    ) -> Vec<(i64, &'a str)> {
        let mut p = Pager::new(after, limit);
        loop {
            let got = store(rows, p.ask());
            if let Some(page) = p.offer(got, |r| r.0, |r| r.1) {
                return page;
            }
        }
    }

    #[test]
    fn every_conversation_of_a_shared_second_is_on_exactly_one_page() {
        let mut rows = vec![(200, "newer")];
        let names: Vec<String> = (0..30).map(|i| format!("same-{i}")).collect();
        rows.extend(names.iter().map(|n| (100, n.as_str())));
        rows.push((50, "older"));
        let mut seen = Vec::new();
        let mut after = None;
        loop {
            let got = page(&rows, after, 4);
            if got.is_empty() {
                break;
            }
            let last = got.last().unwrap();
            after = Some(Cursor::of(last.0, last.1));
            seen.extend(got.into_iter().map(|r| r.1.to_string()));
        }
        let mut want: Vec<String> = rows.iter().map(|r| r.1.to_string()).collect();
        assert_eq!(seen.len(), want.len(), "{seen:?}");
        seen.sort();
        want.sort();
        assert_eq!(seen, want);
    }

    #[test]
    fn a_page_is_ordered_by_time_then_the_second_key() {
        let rows = [(100, "b"), (100, "a"), (100, "c"), (90, "d")];
        let got = page(&rows, None, 10);
        let ords: Vec<i64> = got[..3].iter().map(|r| tid_ord(r.1)).collect();
        let mut sorted = ords.clone();
        sorted.sort();
        assert_eq!(ords, sorted);
        assert_eq!(got[3].1, "d");
    }

    #[test]
    fn a_cursor_reads_back_as_it_was_written() {
        let c = Cursor::of(1_700_000_000, "t-1@example.test");
        assert_eq!(Cursor::parse(&c.encode()), Some(c));
        assert_eq!(Cursor::parse("x:1"), None);
        assert_eq!(Cursor::parse("17"), None);
    }
}
