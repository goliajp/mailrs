//! Fetch subscribed calendar feeds and file their events.
//!
//! Subscribing to an `.ics` URL stored a row and nothing read it, so no
//! event ever appeared. The monolith had a 230-line worker over PG; this is
//! the same job against kevy, using `mailrs_ical::parse::split_vevents` so
//! both read the document the same way.
//!
//! `ETag` / `Last-Modified` are sent back on every poll: a feed that has not
//! changed answers 304 and costs one round trip instead of a re-parse of a
//! year of meetings.

use std::sync::Arc;
use std::time::Duration;

use mailrs_core_sidestate::families::calendar_feeds::{self, FeedRow};

use crate::FastcoreState;

/// How often the loop looks for due feeds when it last found one. Each
/// feed's own interval decides whether it is actually fetched.
const BUSY_INTERVAL: Duration = Duration::from_secs(60);

/// Longest interval when nothing has been due for a while.
///
/// Under half of [`calendar_feeds::MIN_INTERVAL_SECS`], the shortest period a
/// feed may ask to be polled at, so backing off can never make a due feed
/// late by more than a fraction of its own interval. Same number as
/// `webhook_delivery`, which is the other loop of this shape.
const IDLE_INTERVAL: Duration = Duration::from_secs(120);

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Poll subscribed feeds until the process ends.
pub async fn spawn(state: Arc<FastcoreState>) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(err = %e, "calendar sync: no http client, loop not started");
            return;
        }
    };
    tracing::info!("calendar feed sync started");
    let mut idle_rounds = 0u32;
    loop {
        // With no feed subscribed anywhere this used to enumerate every
        // account and read every feed key once a minute, forever, and find
        // nothing — 1440 rounds a day of work that could not accomplish
        // anything. A loop with no cheap resting state is the shape that
        // burned a shared host on 2026-07-19.
        let synced = tick(&state, &client).await;
        idle_rounds = match synced {
            0 => idle_rounds.saturating_add(1),
            _ => 0,
        };
        let wait = crate::idle_backoff::idle_backoff(BUSY_INTERVAL, IDLE_INTERVAL, idle_rounds);
        tokio::time::sleep(wait).await;
    }
}

/// One pass. Returns how many feeds were fetched, which is what tells the
/// loop whether this round accomplished anything.
async fn tick(state: &Arc<FastcoreState>, client: &reqwest::Client) -> usize {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(err = %e, "calendar sync: cannot list accounts");
            return 0;
        }
    };
    let now = now_secs();
    let mut synced = 0usize;
    for user in users {
        let feeds = match read_feeds(state, &user) {
            Some(f) => f,
            None => continue,
        };
        for feed in feeds {
            if !calendar_feeds::is_due(&feed, now) {
                continue;
            }
            let id = feed.id.clone();
            let url = feed.url.clone();
            let updated = sync_one(client, state, &user, feed).await;
            match updated.last_error {
                Some(ref e) => {
                    tracing::warn!(%user, feed = %id, %url, error = %e, "calendar feed sync failed")
                }
                None => tracing::info!(
                    %user,
                    feed = %id,
                    events = updated.last_event_count,
                    "calendar feed synced"
                ),
            }
            write_feed(state, &user, &updated);
            synced += 1;
        }
    }
    synced
}

fn feeds_key(user: &str) -> String {
    format!("calendar_feeds:{user}")
}

fn read_feeds(state: &Arc<FastcoreState>, user: &str) -> Option<Vec<FeedRow>> {
    let mut conn = state.net_conn()?;
    let flat = conn.hgetall(feeds_key(user).as_bytes()).ok()?;
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        if let Ok(row) = serde_json::from_slice::<FeedRow>(&flat[i + 1]) {
            out.push(row);
        }
        i += 2;
    }
    Some(out)
}

fn write_feed(state: &Arc<FastcoreState>, user: &str, row: &FeedRow) {
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let Ok(json) = serde_json::to_vec(row) else {
        return;
    };
    if let Err(e) = conn.hset(
        feeds_key(user).as_bytes(),
        &[(row.id.as_bytes(), json.as_slice())],
    ) {
        tracing::warn!(err = %e, %user, feed = %row.id, "calendar sync: cannot record outcome");
    }
}

/// Fetch one feed and file what it holds. Returns the row to store.
async fn sync_one(
    client: &reqwest::Client,
    state: &Arc<FastcoreState>,
    user: &str,
    feed: FeedRow,
) -> FeedRow {
    let now = now_secs();
    let mut req = client.get(&feed.url);
    if let (Some(u), Some(p)) = (&feed.basic_auth_user, &feed.basic_auth_pass) {
        req = req.basic_auth(u, Some(p));
    }
    if let Some(ref etag) = feed.etag {
        req = req.header("if-none-match", etag);
    }
    if let Some(ref lm) = feed.last_modified {
        req = req.header("if-modified-since", lm);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return calendar_feeds::with_failure(feed, now, &format!("request: {e}")),
    };
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        let (etag, lm, count) = (
            feed.etag.clone(),
            feed.last_modified.clone(),
            feed.last_event_count,
        );
        return calendar_feeds::with_success(feed, now, etag, lm, count);
    }
    if !resp.status().is_success() {
        let msg = format!("HTTP {}", resp.status().as_u16());
        return calendar_feeds::with_failure(feed, now, &msg);
    }

    let etag = header(&resp, reqwest::header::ETAG);
    let last_modified = header(&resp, reqwest::header::LAST_MODIFIED);
    let body = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return calendar_feeds::with_failure(feed, now, &format!("body read: {e}")),
    };
    let Ok(text) = std::str::from_utf8(&body) else {
        // Not an error the user can fix by retrying, and filing zero events
        // silently would look like an empty calendar.
        return calendar_feeds::with_failure(feed, now, "response was not UTF-8");
    };

    let applied = apply(state, user, &feed.id, text);
    calendar_feeds::with_success(feed, now, etag, last_modified, applied as i64)
}

fn header(resp: &reqwest::Response, name: reqwest::header::HeaderName) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// File every event in the document. Returns how many were stored.
fn apply(state: &Arc<FastcoreState>, user: &str, feed_id: &str, text: &str) -> usize {
    let Some(mut conn) = state.net_conn() else {
        return 0;
    };
    let mut applied = 0usize;
    for doc in mailrs_ical::parse::split_vevents(text) {
        let Ok(parsed) = mailrs_ical::parse_invite(doc.as_bytes()) else {
            continue;
        };
        // Zoned starts resolve through the document's own VTIMEZONE blocks;
        // one that resolves to nothing is skipped rather than filed at an
        // invented time.
        let Some(dtstart) =
            mailrs_ical::vtimezone::caldatetime_to_utc(&parsed.dtstart, &parsed.vtimezones)
        else {
            continue;
        };
        let row = serde_json::json!({
            "uid": parsed.uid,
            "summary": parsed.summary,
            "dtstart": dtstart.to_rfc3339(),
            "dtend": parsed.dtend.as_ref().and_then(|d| {
                mailrs_ical::vtimezone::caldatetime_to_utc(d, &parsed.vtimezones)
            }).map(|d| d.to_rfc3339()),
            "organizer": parsed.organizer.as_ref().map(|o| o.email.clone()),
            "status": parsed.status.as_ref().map(|s| format!("{s:?}")),
            // Which subscription put it here, so unsubscribing could remove
            // its events and a hand-made event is never mistaken for one.
            "source": format!("feed:{feed_id}"),
        });
        let Ok(json) = serde_json::to_vec(&row) else {
            continue;
        };
        let key = format!("calendar_event:{user}:{}", parsed.uid);
        if conn
            .hset(key.as_bytes(), &[(b"json".as_slice(), json.as_slice())])
            .is_err()
        {
            continue;
        }
        let index = format!("calendar_events:{user}");
        if conn
            .zadd(
                index.as_bytes(),
                &[(dtstart.timestamp() as f64, parsed.uid.as_bytes())],
            )
            .is_err()
        {
            continue;
        }
        applied += 1;
    }
    applied
}
