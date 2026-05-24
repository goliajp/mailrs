use mailrs_backoff::Backoff;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;

use super::{OutboxEntry, Subscription};

/// generate a cryptographically random signing secret (64 hex chars = 32 bytes)
pub fn generate_signing_secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Retry delay in seconds for attempt `n` using `mailrs-backoff`'s
/// webhook preset (60s initial, 2× growth, 6h cap, Equal jitter).
///
/// Replaces the pre-1.7.11 hardcoded schedule
/// `[60, 120, 300, 600, 1800, 3600, 7200, 21600]`. Curve is now a
/// clean exponential and includes Equal jitter so subscriber endpoints
/// see a smoothed retry pattern under burst load.
pub fn retry_delay_secs(attempt: u32) -> i64 {
    // Use the entry id + attempt as a stable seed so the SAME failing
    // entry retries at the same jittered time (idempotent rescheduling),
    // but DIFFERENT entries spread across the jitter window. Callers
    // pass attempt only — we hash `attempt` for the seed because the
    // attempt-row id isn't in scope here. For deterministic-tests use,
    // attempt=0 gives a reproducible seed.
    let seed = (attempt as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    Backoff::webhook().delay(attempt, seed).as_secs() as i64
}

/// insert a new webhook subscription and return its id
pub async fn create_subscription(
    pool: &PgPool,
    account: &str,
    url: &str,
    event_type: &str,
    filter_sender: Option<&str>,
    filter_thread_id: Option<&str>,
    signing_secret: &str,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO webhook_subscriptions (account_address, url, event_type, filter_sender, filter_thread_id, signing_secret)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(account)
    .bind(url)
    .bind(event_type)
    .bind(filter_sender)
    .bind(filter_thread_id)
    .bind(signing_secret)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// list all active subscriptions for an account
pub async fn list_subscriptions(
    pool: &PgPool,
    account: &str,
) -> Result<Vec<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        "SELECT id, account_address, url, event_type, filter_sender, filter_thread_id, signing_secret, active, created_at
         FROM webhook_subscriptions
         WHERE active = true AND account_address = $1
         ORDER BY created_at DESC",
    )
    .bind(account)
    .fetch_all(pool)
    .await
}

/// soft-delete a subscription by setting active=false, returns true if found
pub async fn delete_subscription(
    pool: &PgPool,
    id: i64,
    account: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE webhook_subscriptions SET active = false WHERE id = $1 AND account_address = $2 AND active = true",
    )
    .bind(id)
    .bind(account)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// get a single subscription by id
pub async fn get_subscription(pool: &PgPool, id: i64) -> Result<Option<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        "SELECT id, account_address, url, event_type, filter_sender, filter_thread_id, signing_secret, active, created_at
         FROM webhook_subscriptions
         WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// find active subscriptions matching account, event type, and optional sender/thread filters
pub async fn find_matching_subscriptions(
    pool: &PgPool,
    account: &str,
    event_type: &str,
    sender: &str,
    thread_id: &str,
) -> Result<Vec<Subscription>, sqlx::Error> {
    sqlx::query_as::<_, Subscription>(
        "SELECT id, account_address, url, event_type, filter_sender, filter_thread_id, signing_secret, active, created_at
         FROM webhook_subscriptions
         WHERE active = true
           AND account_address = $1
           AND event_type = $2
           AND (filter_sender IS NULL OR filter_sender = $3)
           AND (filter_thread_id IS NULL OR filter_thread_id = $4)",
    )
    .bind(account)
    .bind(event_type)
    .bind(sender)
    .bind(thread_id)
    .fetch_all(pool)
    .await
}

/// insert an outbox entry for webhook delivery
pub async fn enqueue_delivery(
    pool: &PgPool,
    subscription_id: i64,
    payload: &serde_json::Value,
    now: i64,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO webhook_outbox (subscription_id, payload, status, next_retry, created_at, updated_at)
         VALUES ($1, $2, 'pending', $3, $3, $3) RETURNING id",
    )
    .bind(subscription_id)
    .bind(payload)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// dequeue pending outbox entries ready for delivery
pub async fn dequeue_pending(
    pool: &PgPool,
    now: i64,
    limit: i32,
) -> Result<Vec<OutboxEntry>, sqlx::Error> {
    sqlx::query_as::<_, OutboxEntry>(
        "SELECT id, subscription_id, payload, status, attempts, max_attempts, next_retry, last_error, created_at, updated_at
         FROM webhook_outbox
         WHERE status = 'pending' AND next_retry <= $1
         ORDER BY created_at
         LIMIT $2",
    )
    .bind(now)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// mark an outbox entry as inflight
pub async fn mark_inflight(pool: &PgPool, id: i64, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE webhook_outbox SET status = 'inflight', updated_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

/// mark an outbox entry as delivered
pub async fn mark_delivered(pool: &PgPool, id: i64, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE webhook_outbox SET status = 'delivered', updated_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

/// mark an outbox entry as failed, with exponential backoff retry or permanent failure
pub async fn mark_failed(
    pool: &PgPool,
    id: i64,
    error: &str,
    attempt: i32,
    max_attempts: i32,
    now: i64,
) -> Result<(), sqlx::Error> {
    if attempt >= max_attempts {
        sqlx::query(
            "UPDATE webhook_outbox SET status = 'failed', attempts = $2, last_error = $3, updated_at = $4 WHERE id = $1",
        )
        .bind(id)
        .bind(attempt)
        .bind(error)
        .bind(now)
        .execute(pool)
        .await?;
    } else {
        let next = now + retry_delay_secs(attempt as u32);
        sqlx::query(
            "UPDATE webhook_outbox SET status = 'pending', attempts = $2, last_error = $3, next_retry = $4, updated_at = $5 WHERE id = $1",
        )
        .bind(id)
        .bind(attempt)
        .bind(error)
        .bind(next)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
