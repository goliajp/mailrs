//! Background worker that polls external ICS feeds (MRS-10).
//!
//! On each tick (default 60s) it pulls feeds whose
//! `last_synced_at + refresh_interval_secs < now` from PG, fires GETs
//! with HTTP cache validators (`If-None-Match` / `If-Modified-Since`),
//! parses 200 responses via [`mailrs_ical::parse_invite`] for each
//! `BEGIN:VEVENT...END:VEVENT` block, and upserts them into the feed's
//! dedicated read-only calendar. 304 responses skip parsing entirely.
//!
//! Errors per-feed are recorded back into `external_calendar_feeds.last_error`
//! so the settings UI can surface them without the user having to dig
//! through server logs.

use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;

use super::feed::{record_error, record_success, ExternalFeed};

/// Tick cadence. Individual feeds choose their own refresh_interval
/// (default 15min); the worker just wakes up frequently enough to
/// notice when one is due.
const WORKER_TICK: Duration = Duration::from_secs(60);

/// Spawn the long-lived feed worker. Cheap if there are no feeds (the
/// DUE query just returns empty and we sleep).
pub fn spawn_feed_worker(pool: PgPool) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = run_one_tick(&pool).await {
                tracing::warn!("external feed worker tick error: {e}");
            }
            tokio::time::sleep(WORKER_TICK).await;
        }
    });
}

async fn run_one_tick(pool: &PgPool) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    let feeds = super::feed::list_due(pool, now).await?;
    if feeds.is_empty() {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest client");

    for feed in feeds {
        let outcome = sync_one(&client, pool, &feed).await;
        let now = Utc::now();
        match outcome {
            Ok(SyncOutcome::Updated { etag, last_modified }) => {
                let _ = record_success(pool, feed.id, now, etag.as_deref(), last_modified.as_deref())
                    .await;
            }
            Ok(SyncOutcome::NotModified) => {
                let _ = record_success(pool, feed.id, now, feed.etag.as_deref(), feed.last_modified.as_deref())
                    .await;
            }
            Err(e) => {
                let _ = record_error(pool, feed.id, now, &e).await;
                tracing::warn!(feed_id = feed.id, url = %feed.url, error = %e, "feed sync failed");
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
enum SyncOutcome {
    Updated {
        etag: Option<String>,
        last_modified: Option<String>,
    },
    NotModified,
}

async fn sync_one(
    client: &reqwest::Client,
    pool: &PgPool,
    feed: &ExternalFeed,
) -> Result<SyncOutcome, String> {
    let mut req = client.get(&feed.url);
    if let (Some(u), Some(p)) = (&feed.basic_auth_user, &feed.basic_auth_pass) {
        req = req.basic_auth(u, Some(p));
    }
    if let Some(etag) = &feed.etag {
        req = req.header("if-none-match", etag);
    }
    if let Some(lm) = &feed.last_modified {
        req = req.header("if-modified-since", lm);
    }

    let resp = req.send().await.map_err(|e| format!("request: {e}"))?;
    let status = resp.status();

    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(SyncOutcome::NotModified);
    }
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }

    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let last_modified = resp
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("body read: {e}"))?;

    apply_ics_to_calendar(pool, feed.calendar_id, &body)
        .await
        .map_err(|e| format!("apply: {e}"))?;

    Ok(SyncOutcome::Updated {
        etag,
        last_modified,
    })
}

/// Walk every VEVENT in the fetched .ics and upsert it into the feed's
/// calendar. Uses the existing [`super::event::upsert_from_parsed_invite`]
/// path so structured columns (organizer, attendees, RRULE, ...) stay
/// consistent with the rest of mailrs.
async fn apply_ics_to_calendar(
    pool: &PgPool,
    calendar_id: i64,
    body: &[u8],
) -> Result<usize, sqlx::Error> {
    let text = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(_) => return Ok(0),
    };

    // Split the document on its outer VEVENT blocks rather than re-parse
    // the whole VCALENDAR per event. We walk: locate each
    // BEGIN:VEVENT...END:VEVENT span, wrap with a minimal VCALENDAR, hand
    // to mailrs_ical::parse_invite.
    let mut applied = 0usize;
    let mut search_from = 0;
    while let Some(begin_rel) = text[search_from..].find("BEGIN:VEVENT") {
        let begin = search_from + begin_rel;
        let Some(end_rel) = text[begin..].find("END:VEVENT") else {
            break;
        };
        let end = begin + end_rel + "END:VEVENT".len();
        let event_body = &text[begin..end];

        let wrapped = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//mailrs//feed-import//EN\r\n{event_body}\r\nEND:VCALENDAR\r\n",
        );

        if let Ok(parsed) = mailrs_ical::parse_invite(wrapped.as_bytes()) {
            let etag = format!("{:x}", parsed.dtstamp.timestamp_micros());
            // Use parsed.uid as the calendar key; same UID across syncs
            // overwrites in place via the partial-index conflict target.
            super::event::upsert_from_parsed_invite(
                pool,
                calendar_id,
                &parsed.uid,
                &parsed,
                &wrapped,
                &etag,
            )
            .await?;
            applied += 1;
        }

        search_from = end;
    }

    Ok(applied)
}

#[cfg(test)]
#[path = "feed_worker_tests.rs"]
mod tests;
