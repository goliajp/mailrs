//! How a conversation list request pages, decided once for every core.
//!
//! `before_ts` pages by time alone and stays for the clients that send it;
//! `after_cursor` pages by time and a second key ([`mailrs_mailbox::page_cursor`]),
//! and so does a request that names neither, so the cursor on its last row
//! is one the next request can follow.

use mailrs_mailbox::page_cursor::{Cursor, Pager};

use crate::types::{ConversationFilter, ConversationSummaryWire};

/// What a list request asked for.
pub enum Paging {
    /// The old way: rows with a time below this, as many as fit.
    BeforeTs(i64),
    /// By cursor, in rounds until the boundary second is complete.
    Cursor(Pager),
}

/// Why a list request cannot be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagingError {
    /// `before_ts` and `after_cursor` both given.
    Both,
    /// `after_cursor` is not a cursor a list gave out.
    BadCursor,
}

impl ConversationFilter {
    /// Whether this list orders and pages by when the reader began
    /// waiting rather than by last activity: the awaiting section.
    pub fn pages_by_awaiting(&self) -> bool {
        self.section
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("awaiting"))
    }

    /// How this request pages, for a page of `limit` rows.
    pub fn paging(&self, limit: usize) -> Result<Paging, PagingError> {
        match (self.before_ts, self.after_cursor.as_deref()) {
            (Some(_), Some(_)) => Err(PagingError::Both),
            (Some(ts), None) => Ok(Paging::BeforeTs(ts)),
            (None, Some(c)) => match Cursor::parse(c) {
                Some(after) => Ok(Paging::Cursor(Pager::new(Some(after), limit))),
                None => Err(PagingError::BadCursor),
            },
            (None, None) => Ok(Paging::Cursor(Pager::new(None, limit))),
        }
    }

    /// The time `row` is ordered and paged by in this list.
    pub fn page_time(&self, row: &ConversationSummaryWire) -> i64 {
        match self.pages_by_awaiting() {
            true => row.awaiting_since,
            false => row.last_date,
        }
    }

    /// Write each row's cursor.
    pub fn stamp_cursors(&self, rows: &mut [ConversationSummaryWire]) {
        for row in rows {
            row.cursor = Cursor::of(self.page_time(row), &row.thread_id).encode();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_pages_one_way_only() {
        let both = ConversationFilter {
            before_ts: Some(10),
            after_cursor: Some("10:1".into()),
            ..Default::default()
        };
        assert!(matches!(both.paging(5), Err(PagingError::Both)));
        let bad = ConversationFilter {
            after_cursor: Some("ten".into()),
            ..Default::default()
        };
        assert!(matches!(bad.paging(5), Err(PagingError::BadCursor)));
        let old = ConversationFilter {
            before_ts: Some(10),
            ..Default::default()
        };
        assert!(matches!(old.paging(5), Ok(Paging::BeforeTs(10))));
        assert!(matches!(
            ConversationFilter::default().paging(5),
            Ok(Paging::Cursor(_))
        ));
    }
}
