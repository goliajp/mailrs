//! Subscribed calendar feeds on kevy: the row, and when it is due.
//!
//! ```text
//!   calendar_feeds:{user}          hash  feed_id → JSON FeedRow
//!   calendar_events:{user}         zset  score = dtstart epoch, member = uid
//!   calendar_event:{user}:{uid}    hash  the event
//! ```
//!
//! The CRUD existed and nothing read it: subscribing to an `.ics` URL stored
//! a row and no events ever appeared. The monolith had a 230-line worker
//! over PG. What was missing here is the sync state a worker needs — when it
//! last ran, what went wrong, and the validators that make a poll cheap.
//!
//! `last_error` is a stored field rather than a log line because the user
//! subscribed to the feed and is the one who can fix a wrong URL or a
//! rotated password. A feed that has been failing for a week should say so
//! on the page where it was added.

use serde::{Deserialize, Serialize};

/// A subscribed feed, as stored.
///
/// The sync fields default, so rows written before this existed load and are
/// treated as never-synced rather than failing to parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedRow {
    /// Opaque id, unique within the user's hash.
    pub id: String,
    /// What the user called it.
    pub name: String,
    /// Where to fetch.
    pub url: String,
    /// Display colour.
    #[serde(default)]
    pub color: Option<String>,
    /// Requested seconds between polls.
    #[serde(default)]
    pub sync_interval_secs: i64,
    /// When the subscription was created.
    #[serde(default)]
    pub created_at: i64,
    /// When a fetch last succeeded. Zero means never.
    #[serde(default)]
    pub last_synced_at: i64,
    /// Why the last attempt failed, if it did. Cleared by a success.
    #[serde(default)]
    pub last_error: Option<String>,
    /// How many events the last successful sync applied.
    #[serde(default)]
    pub last_event_count: i64,
    /// `ETag` from the last successful fetch, for `If-None-Match`.
    #[serde(default)]
    pub etag: Option<String>,
    /// `Last-Modified` from the last successful fetch.
    #[serde(default)]
    pub last_modified: Option<String>,
    /// Username for a feed behind HTTP basic auth.
    #[serde(default)]
    pub basic_auth_user: Option<String>,
    /// Password for a feed behind HTTP basic auth.
    #[serde(default)]
    pub basic_auth_pass: Option<String>,
}

/// Shortest interval a feed may ask for.
///
/// A subscriber that sets zero — or omits the field, which deserializes to
/// zero — would otherwise be polled on every tick forever, against someone
/// else's server.
pub const MIN_INTERVAL_SECS: i64 = 300;

/// Interval used when the row does not name a usable one.
pub const DEFAULT_INTERVAL_SECS: i64 = 3600;

/// The interval this feed is actually polled at.
pub fn effective_interval(row: &FeedRow) -> i64 {
    match row.sync_interval_secs {
        n if n <= 0 => DEFAULT_INTERVAL_SECS,
        n => n.max(MIN_INTERVAL_SECS),
    }
}

/// Whether the feed should be fetched now.
///
/// A never-synced feed is due immediately, so subscribing produces events
/// without waiting out an hour. A failing feed is not retried faster than a
/// working one — the remote is already unhappy.
pub fn is_due(row: &FeedRow, now: i64) -> bool {
    match row.last_synced_at {
        0 => true,
        last => now - last >= effective_interval(row),
    }
}

/// The row after a successful fetch.
pub fn with_success(
    mut row: FeedRow,
    now: i64,
    etag: Option<String>,
    last_modified: Option<String>,
    event_count: i64,
) -> FeedRow {
    row.last_synced_at = now;
    // Cleared, so a feed that recovered does not keep showing last week's
    // failure next to a fresh timestamp.
    row.last_error = None;
    row.last_event_count = event_count;
    row.etag = etag;
    row.last_modified = last_modified;
    row
}

/// The row after a failed fetch.
///
/// `last_synced_at` advances even though nothing synced: it is the poll
/// clock, and leaving it would retry a broken feed on every tick. The
/// failure is visible in `last_error`, which is where it belongs.
pub fn with_failure(mut row: FeedRow, now: i64, error: &str) -> FeedRow {
    row.last_synced_at = now;
    row.last_error = Some(error.to_string());
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> FeedRow {
        FeedRow {
            id: "f1".into(),
            name: "Team".into(),
            url: "https://example.com/cal.ics".into(),
            color: None,
            sync_interval_secs: 3600,
            created_at: 0,
            last_synced_at: 0,
            last_error: None,
            last_event_count: 0,
            etag: None,
            last_modified: None,
            basic_auth_user: None,
            basic_auth_pass: None,
        }
    }

    #[test]
    fn a_new_subscription_is_due_at_once() {
        assert!(is_due(&row(), 1_000_000));
    }

    #[test]
    fn a_synced_feed_waits_out_its_interval() {
        let mut r = row();
        r.last_synced_at = 1_000_000;
        assert!(!is_due(&r, 1_000_000 + 3599));
        assert!(is_due(&r, 1_000_000 + 3600));
    }

    /// Zero would mean "every tick", against someone else's server.
    #[test]
    fn an_absent_or_tiny_interval_is_raised() {
        let mut r = row();
        r.sync_interval_secs = 0;
        assert_eq!(effective_interval(&r), DEFAULT_INTERVAL_SECS);
        r.sync_interval_secs = -5;
        assert_eq!(effective_interval(&r), DEFAULT_INTERVAL_SECS);
        r.sync_interval_secs = 10;
        assert_eq!(effective_interval(&r), MIN_INTERVAL_SECS);
    }

    #[test]
    fn success_clears_the_previous_error() {
        let mut r = row();
        r = with_failure(r, 100, "HTTP 401");
        assert_eq!(r.last_error.as_deref(), Some("HTTP 401"));
        r = with_success(r, 200, Some("W/\"x\"".into()), None, 12);
        assert_eq!(r.last_error, None);
        assert_eq!(r.last_event_count, 12);
        assert_eq!(r.etag.as_deref(), Some("W/\"x\""));
        assert_eq!(r.last_synced_at, 200);
    }

    /// A failing feed must not be retried on every tick.
    #[test]
    fn a_failure_still_advances_the_poll_clock() {
        let r = with_failure(row(), 1_000_000, "request: timed out");
        assert!(!is_due(&r, 1_000_000 + 10));
        assert!(is_due(&r, 1_000_000 + 3600));
    }

    /// Rows written before the sync fields existed must still load.
    #[test]
    fn an_old_row_without_sync_fields_parses_as_never_synced() {
        let json = r#"{"id":"f1","name":"Team","url":"https://x/c.ics","created_at":5}"#;
        let r: FeedRow = serde_json::from_str(json).expect("parses");
        assert_eq!(r.last_synced_at, 0);
        assert!(is_due(&r, 10));
        assert_eq!(effective_interval(&r), DEFAULT_INTERVAL_SECS);
    }
}
