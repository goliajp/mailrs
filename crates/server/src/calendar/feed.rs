//! External WebDAV-hosted ICS feed subscription (MRS-10).
//!
//! Stores per-account subscriptions to remote `.ics` URLs (room
//! calendars, public team calendars, personal exports). A background
//! worker walks active feeds at their refresh interval, fetches with
//! cache validators, parses via [`crate::ical`], and upserts events
//! into a per-feed read-only calendar. The user's CalDAV-subscribed
//! native client picks them up alongside their own events.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)] // FromRow forces every column; not every caller reads all
pub struct ExternalFeed {
    pub id: i64,
    pub account_address: String,
    pub calendar_id: i64,
    pub url: String,
    pub name: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub basic_auth_user: Option<String>,
    pub basic_auth_pass: Option<String>,
    pub refresh_interval_secs: i32,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateFeed<'a> {
    pub account_address: &'a str,
    pub url: &'a str,
    pub name: &'a str,
    pub basic_auth_user: Option<&'a str>,
    pub basic_auth_pass: Option<&'a str>,
    pub refresh_interval_secs: i32,
}

/// Create a new feed subscription, allocating a dedicated read-only
/// calendar for it. The calendar's name defaults to the feed name (or
/// the URL host if name is empty).
pub async fn create(pool: &PgPool, req: CreateFeed<'_>) -> Result<i64, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let display_name = if req.name.is_empty() {
        url_host_or_url(req.url)
    } else {
        req.name.to_string()
    };

    // Allocate the backing read-only calendar.
    let calendar_id: i64 = sqlx::query_scalar(
        "INSERT INTO calendars (account_address, name, description, is_external_readonly)
         VALUES ($1, $2, $3, TRUE)
         RETURNING id",
    )
    .bind(req.account_address)
    .bind(&display_name)
    .bind(format!("Subscribed: {}", req.url))
    .fetch_one(&mut *tx)
    .await?;

    let feed_id: i64 = sqlx::query_scalar(
        "INSERT INTO external_calendar_feeds
            (account_address, calendar_id, url, name,
             basic_auth_user, basic_auth_pass, refresh_interval_secs)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id",
    )
    .bind(req.account_address)
    .bind(calendar_id)
    .bind(req.url)
    .bind(&display_name)
    .bind(req.basic_auth_user)
    .bind(req.basic_auth_pass)
    .bind(req.refresh_interval_secs)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(feed_id)
}

pub async fn list_for_account(
    pool: &PgPool,
    account: &str,
) -> Result<Vec<ExternalFeed>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM external_calendar_feeds WHERE account_address = $1 ORDER BY id ASC")
        .bind(account)
        .fetch_all(pool)
        .await
}

pub async fn delete(pool: &PgPool, account: &str, feed_id: i64) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM external_calendar_feeds WHERE id = $1 AND account_address = $2",
    )
    .bind(feed_id)
    .bind(account)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Fetch all feeds whose `enabled = TRUE` and that are due for sync
/// (last_synced_at IS NULL OR last_synced_at + interval < now). Used by
/// the background worker on each tick.
pub async fn list_due(pool: &PgPool, now: DateTime<Utc>) -> Result<Vec<ExternalFeed>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM external_calendar_feeds
         WHERE enabled = TRUE
           AND (last_synced_at IS NULL
                OR last_synced_at + (refresh_interval_secs || ' seconds')::interval < $1)
         ORDER BY last_synced_at ASC NULLS FIRST
         LIMIT 50",
    )
    .bind(now)
    .fetch_all(pool)
    .await
}

pub async fn record_success(
    pool: &PgPool,
    feed_id: i64,
    now: DateTime<Utc>,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE external_calendar_feeds
         SET last_synced_at = $1, etag = $2, last_modified = $3,
             last_error = NULL, updated_at = now()
         WHERE id = $4",
    )
    .bind(now)
    .bind(etag)
    .bind(last_modified)
    .bind(feed_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn record_error(
    pool: &PgPool,
    feed_id: i64,
    now: DateTime<Utc>,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE external_calendar_feeds
         SET last_synced_at = $1, last_error = $2, updated_at = now()
         WHERE id = $3",
    )
    .bind(now)
    .bind(error)
    .bind(feed_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn url_host_or_url(url: &str) -> String {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return host.to_string();
        }
    }
    url.to_string()
}
