use redis::AsyncCommands;
use sqlx::PgPool;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueStatus {
    Pending,
    InFlight,
    Delivered,
    Failed,
    Bounced,
}

impl QueueStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InFlight => "inflight",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
            Self::Bounced => "bounced",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "inflight" => Some(Self::InFlight),
            "delivered" => Some(Self::Delivered),
            "failed" => Some(Self::Failed),
            "bounced" => Some(Self::Bounced),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub id: i64,
    pub sender: String,
    pub recipient: String,
    pub domain: String,
    pub message_data: Vec<u8>,
    pub status: QueueStatus,
    pub attempts: u32,
    pub max_attempts: u32,
    pub next_retry: i64,
    pub last_error: Option<String>,
    pub message_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_forwarded: bool,
}

/// enqueue a message for outbound delivery
pub async fn enqueue(
    pool: &PgPool,
    sender: &str,
    recipient: &str,
    domain: &str,
    message_data: &[u8],
    message_id: Option<&str>,
    now: i64,
) -> Result<i64, sqlx::Error> {
    enqueue_ex(pool, sender, recipient, domain, message_data, message_id, now, false).await
}

/// enqueue a message for outbound delivery with forwarding flag
pub async fn enqueue_ex(
    pool: &PgPool,
    sender: &str,
    recipient: &str,
    domain: &str,
    message_data: &[u8],
    message_id: Option<&str>,
    now: i64,
    is_forwarded: bool,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO outbound_queue (sender, recipient, domain, message_data, status, next_retry, message_id, created_at, updated_at, is_forwarded)
         VALUES ($1, $2, $3, $4, 'pending', $5, $6, $5, $5, $7)
         RETURNING id",
    )
    .bind(sender)
    .bind(recipient)
    .bind(domain)
    .bind(message_data)
    .bind(now)
    .bind(message_id)
    .bind(is_forwarded)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// notify the delivery worker that new messages are queued
pub async fn notify(valkey: &mut redis::aio::ConnectionManager) {
    let _: Result<i32, _> = valkey.publish("queue:notify", "1").await;
}

/// fetch pending messages ready for delivery
pub async fn dequeue(pool: &PgPool, now: i64, limit: u32) -> Result<Vec<QueuedMessage>, sqlx::Error> {
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

    Ok(rows.into_iter().map(|r| QueuedMessage {
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
    }).collect())
}

/// mark a message as in-flight
pub async fn mark_inflight(pool: &PgPool, id: i64, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE outbound_queue SET status = 'inflight', updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// mark a message as delivered
pub async fn mark_delivered(pool: &PgPool, id: i64, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE outbound_queue SET status = 'delivered', updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// mark a message as failed with next retry time
pub async fn mark_failed(
    pool: &PgPool,
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
pub async fn mark_bounced(pool: &PgPool, id: i64, error: &str, now: i64) -> Result<(), sqlx::Error> {
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

/// get queue statistics
pub async fn queue_stats(pool: &PgPool) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, COUNT(*) FROM outbound_queue GROUP BY status",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// get a specific queued message by id
pub async fn get_message(pool: &PgPool, id: i64) -> Result<Option<QueuedMessage>, sqlx::Error> {
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

/// reset a bounced/failed message back to pending for retry
pub async fn retry_message(pool: &PgPool, id: i64, now: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE outbound_queue SET status = 'pending', next_retry = $1, updated_at = $1 WHERE id = $2 AND status IN ('bounced', 'failed')",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

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

/// list recent queue entries for admin UI
pub async fn list_recent(pool: &PgPool, limit: i32) -> Result<Vec<QueuedMessage>, sqlx::Error> {
    let rows: Vec<(i64, String, String, String, Vec<u8>, String, i32, i32, i64, Option<String>, Option<String>, i64, i64, bool)> = sqlx::query_as(
        "SELECT id, sender, recipient, domain, message_data, status, attempts, max_attempts, next_retry, last_error, message_id, created_at, updated_at, is_forwarded
         FROM outbound_queue
         ORDER BY created_at DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| QueuedMessage {
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
    }).collect())
}
