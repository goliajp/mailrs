//! Everything the admin surface asks of the queue: counts, listings,
//! cancel, retry, reschedule, and the suppression list.

use super::*;
#[cfg(feature = "pg")]
use crate::BackendPool;

/// get queue statistics
#[cfg(feature = "pg")]
pub async fn queue_stats(pool: &BackendPool) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, COUNT(*) FROM outbound_queue GROUP BY status")
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

/// Count of rows in `status = 'pending'` (any `next_retry`). Used by
/// the delivery worker to publish a `mailrs_outbound_queue_depth` gauge
/// per poll tick. O(rows) but cheap with the `status` index.
#[cfg(feature = "pg")]
pub async fn count_pending(pool: &BackendPool) -> Result<i64, sqlx::Error> {
    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM outbound_queue WHERE status = 'pending'")
            .fetch_one(pool)
            .await?;
    Ok(n)
}

/// Count of rows in `status = 'inflight'` (currently being delivered
/// by a worker). Used alongside `count_pending` so dashboards can show
/// both "queued" and "in-flight" depth.
#[cfg(feature = "pg")]
pub async fn count_inflight(pool: &BackendPool) -> Result<i64, sqlx::Error> {
    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM outbound_queue WHERE status = 'inflight'")
            .fetch_one(pool)
            .await?;
    Ok(n)
}

/// get a specific queued message by id
#[cfg(feature = "pg")]
pub async fn get_message(
    pool: &BackendPool,
    id: i64,
) -> Result<Option<QueuedMessage>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let row: Option<(i64, String, String, String, Vec<u8>, String, i32, i32, i64, Option<String>, Option<String>, i64, i64, bool)> = sqlx::query_as(
        "SELECT id, sender, recipient, domain, message_data, status, attempts, max_attempts, next_retry, last_error, message_id, created_at, updated_at, is_forwarded
         FROM outbound_queue WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| QueuedMessage {
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
    }))
}

/// list recent queue entries for admin UI
#[cfg(feature = "pg")]
pub async fn list_recent(
    pool: &BackendPool,
    limit: i32,
) -> Result<Vec<QueuedMessage>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, String, String, Vec<u8>, String, i32, i32, i64, Option<String>, Option<String>, i64, i64, bool)> = sqlx::query_as(
        "SELECT id, sender, recipient, domain, message_data, status, attempts, max_attempts, next_retry, last_error, message_id, created_at, updated_at, is_forwarded
         FROM outbound_queue
         ORDER BY created_at DESC
         LIMIT $1",
    )
    .bind(limit)
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

/// cancel a pending outbound message (undo send)
#[cfg(feature = "pg")]
pub async fn cancel_pending(pool: &BackendPool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM outbound_queue WHERE id = $1 AND status = 'pending'")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// cancel a pending outbound message by message_id (undo send)
#[cfg(feature = "pg")]
pub async fn cancel_pending_by_message_id(
    pool: &BackendPool,
    message_id: &str,
    sender: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM outbound_queue WHERE message_id = $1 AND status = 'pending' AND sender = $2",
    )
    .bind(message_id)
    .bind(sender)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// reset a bounced/failed message back to pending for retry
#[cfg(feature = "pg")]
pub async fn retry_message(pool: &BackendPool, id: i64, now: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE outbound_queue SET status = 'pending', next_retry = $1, updated_at = $1 WHERE id = $2 AND status IN ('bounced', 'failed')",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// move a still-pending message's delivery time to `scheduled_at`
#[cfg(feature = "pg")]
pub async fn reschedule_pending(
    pool: &BackendPool,
    id: i64,
    scheduled_at: i64,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE outbound_queue SET next_retry = $1, updated_at = $2 WHERE id = $3 AND status = 'pending'",
    )
    .bind(scheduled_at)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub use suppression::is_hard_bounce;
#[cfg(feature = "pg")]
pub use suppression::{add_suppression, is_suppressed, list_suppressions, remove_suppression};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_status_roundtrip() {
        let variants = [
            QueueStatus::Pending,
            QueueStatus::InFlight,
            QueueStatus::Delivered,
            QueueStatus::Failed,
            QueueStatus::Bounced,
        ];
        for v in &variants {
            let s = v.as_str();
            let parsed = QueueStatus::parse(s).unwrap();
            assert_eq!(&parsed, v, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn queue_status_parse_unknown() {
        assert_eq!(QueueStatus::parse("unknown"), None);
        assert_eq!(QueueStatus::parse(""), None);
        assert_eq!(QueueStatus::parse("PENDING"), None);
    }

    #[test]
    fn queue_status_as_str_values() {
        assert_eq!(QueueStatus::Pending.as_str(), "pending");
        assert_eq!(QueueStatus::InFlight.as_str(), "inflight");
        assert_eq!(QueueStatus::Delivered.as_str(), "delivered");
        assert_eq!(QueueStatus::Failed.as_str(), "failed");
        assert_eq!(QueueStatus::Bounced.as_str(), "bounced");
    }

    #[test]
    fn queue_status_parse_case_sensitive() {
        // parse is case-sensitive — uppercase variants are not valid
        assert_eq!(QueueStatus::parse("Pending"), None);
        assert_eq!(QueueStatus::parse("InFlight"), None);
        assert_eq!(QueueStatus::parse("DELIVERED"), None);
        assert_eq!(QueueStatus::parse("Failed"), None);
        assert_eq!(QueueStatus::parse("Bounced"), None);
    }

    #[test]
    fn queue_status_parse_whitespace_rejected() {
        assert_eq!(QueueStatus::parse(" pending"), None);
        assert_eq!(QueueStatus::parse("pending "), None);
        assert_eq!(QueueStatus::parse("  "), None);
    }

    #[test]
    fn queue_status_eq() {
        assert_eq!(QueueStatus::Pending, QueueStatus::Pending);
        assert_ne!(QueueStatus::Pending, QueueStatus::Delivered);
        assert_ne!(QueueStatus::Failed, QueueStatus::Bounced);
    }

    #[test]
    fn queue_status_clone() {
        let s = QueueStatus::InFlight;
        let c = s.clone();
        assert_eq!(s, c);
    }

    #[test]
    fn queued_message_clone_preserves_fields() {
        let msg = QueuedMessage {
            id: 42,
            sender: "s@example.com".into(),
            recipient: "r@remote.com".into(),
            domain: "remote.com".into(),
            message_data: vec![1, 2, 3],
            status: QueueStatus::Pending,
            attempts: 3,
            max_attempts: 8,
            next_retry: 1_700_000_000,
            last_error: Some("temporary failure".into()),
            message_id: Some("msg-id-123".into()),
            created_at: 1_699_000_000,
            updated_at: 1_699_500_000,
            is_forwarded: true,
        };
        let cloned = msg.clone();
        assert_eq!(cloned.id, 42);
        assert_eq!(cloned.sender, "s@example.com");
        assert_eq!(cloned.recipient, "r@remote.com");
        assert_eq!(cloned.domain, "remote.com");
        assert_eq!(cloned.message_data, vec![1, 2, 3]);
        assert_eq!(cloned.attempts, 3);
        assert_eq!(cloned.max_attempts, 8);
        assert_eq!(cloned.next_retry, 1_700_000_000);
        assert_eq!(cloned.last_error, Some("temporary failure".into()));
        assert_eq!(cloned.message_id, Some("msg-id-123".into()));
        assert!(cloned.is_forwarded);
    }

    #[test]
    fn queued_message_no_last_error() {
        let msg = QueuedMessage {
            id: 1,
            sender: "s@example.com".into(),
            recipient: "r@remote.com".into(),
            domain: "remote.com".into(),
            message_data: vec![],
            status: QueueStatus::Pending,
            attempts: 0,
            max_attempts: 8,
            next_retry: 0,
            last_error: None,
            message_id: None,
            created_at: 0,
            updated_at: 0,
            is_forwarded: false,
        };
        assert!(msg.last_error.is_none());
        assert!(msg.message_id.is_none());
    }
}
