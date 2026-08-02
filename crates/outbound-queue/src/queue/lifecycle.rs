//! A message's path through the queue: claim, then one of the four
//! terminal marks, plus the sweep that reclaims a crashed claim.

use super::*;
#[cfg(feature = "pg")]
use crate::BackendPool;

/// fetch pending messages ready for delivery (legacy, non-atomic).
///
/// Returns rows in `status = 'pending'` without locking or marking
/// them. Multi-worker setups using this then-`mark_inflight` flow can
/// race on the same row and deliver twice. Prefer
/// [`claim_for_delivery`] for any worker that may run with siblings.
/// Kept for callers that only need a read snapshot.
#[cfg(feature = "pg")]
pub async fn dequeue(
    pool: &BackendPool,
    now: i64,
    limit: u32,
) -> Result<Vec<QueuedMessage>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, String, String, Vec<u8>, String, i32, i32, i64, Option<String>, Option<String>, i64, i64, bool)> = sqlx::query_as(
        "SELECT id, sender, recipient, domain, message_data, status, attempts, max_attempts, next_retry, last_error, message_id, created_at, updated_at, is_forwarded
         FROM outbound_queue
         WHERE status = 'pending' AND next_retry <= $1
         ORDER BY next_retry ASC
         LIMIT $2",
    )
    .bind(now)
    .bind(limit as i32)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| QueuedMessage {
            id: r.0,
            sender: r.1,
            recipient: r.2,
            domain: r.3,
            message_data: r.4,
            status: QueueStatus::parse(&r.5).unwrap_or(QueueStatus::Pending),
            attempts: r.6 as u32,
            max_attempts: r.7 as u32,
            next_retry: r.8,
            last_error: r.9,
            message_id: r.10,
            created_at: r.11,
            updated_at: r.12,
            is_forwarded: r.13,
        })
        .collect())
}

/// Atomically claim up to `limit` pending messages and transition
/// them to `inflight`, returning the claimed rows.
///
/// Implemented as `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP
/// LOCKED) RETURNING …` so concurrent workers never pick the same
/// row (SKIP LOCKED skips rows already locked by other workers),
/// and the claim+transition collapse to a single round-trip and
/// single WAL fsync per batch instead of one SELECT + N per-row
/// UPDATEs.
///
/// Correctness vs the legacy `dequeue` + per-row `mark_inflight`
/// flow: SKIP LOCKED prevents the duplicate-delivery race that the
/// pre-existing flow exposed when more than one worker was running
/// concurrently (both workers would `SELECT` the same pending rows
/// and both proceed past their racing `UPDATE`s). With this claim
/// path, each pending row is delivered by at most one worker.
#[cfg(feature = "pg")]
pub async fn claim_for_delivery(
    pool: &BackendPool,
    now: i64,
    limit: u32,
) -> Result<Vec<QueuedMessage>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, String, String, Vec<u8>, String, i32, i32, i64, Option<String>, Option<String>, i64, i64, bool)> = sqlx::query_as(
        "UPDATE outbound_queue
         SET status = 'inflight', updated_at = $1
         WHERE id IN (
           SELECT id FROM outbound_queue
           WHERE status = 'pending' AND next_retry <= $2
           ORDER BY next_retry ASC
           LIMIT $3
           FOR UPDATE SKIP LOCKED
         )
         RETURNING id, sender, recipient, domain, message_data, status, attempts, max_attempts, next_retry, last_error, message_id, created_at, updated_at, is_forwarded",
    )
    .bind(now)
    .bind(now)
    .bind(limit as i32)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| QueuedMessage {
            id: r.0,
            sender: r.1,
            recipient: r.2,
            domain: r.3,
            message_data: r.4,
            status: QueueStatus::parse(&r.5).unwrap_or(QueueStatus::InFlight),
            attempts: r.6 as u32,
            max_attempts: r.7 as u32,
            next_retry: r.8,
            last_error: r.9,
            message_id: r.10,
            created_at: r.11,
            updated_at: r.12,
            is_forwarded: r.13,
        })
        .collect())
}

/// mark a message as in-flight
#[cfg(feature = "pg")]
pub async fn mark_inflight(pool: &BackendPool, id: i64, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE outbound_queue SET status = 'inflight', updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// mark a message as delivered
#[cfg(feature = "pg")]
pub async fn mark_delivered(pool: &BackendPool, id: i64, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE outbound_queue SET status = 'delivered', updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// mark a message as failed with next retry time
#[cfg(feature = "pg")]
pub async fn mark_failed(
    pool: &BackendPool,
    id: i64,
    error: &str,
    next_retry: i64,
    now: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE outbound_queue SET status = 'pending', attempts = attempts + 1, last_error = $1, next_retry = $2, updated_at = $3 WHERE id = $4",
    )
    .bind(error)
    .bind(next_retry)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// mark a message as permanently bounced
#[cfg(feature = "pg")]
pub async fn mark_bounced(
    pool: &BackendPool,
    id: i64,
    error: &str,
    now: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE outbound_queue SET status = 'bounced', last_error = $1, updated_at = $2 WHERE id = $3",
    )
    .bind(error)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// recover messages stuck in inflight status for more than 10 minutes
/// (worker crashed or was killed before marking them as delivered/failed)
#[cfg(feature = "pg")]
pub async fn recover_stale_inflight(pool: &BackendPool, now: i64) -> Result<u64, sqlx::Error> {
    let stale_threshold = now - 600; // 10 minutes
    let result = sqlx::query(
        "UPDATE outbound_queue SET status = 'pending', updated_at = $1 \
         WHERE status = 'inflight' AND updated_at < $2",
    )
    .bind(now)
    .bind(stale_threshold)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
