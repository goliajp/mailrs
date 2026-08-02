#[cfg(feature = "pg")]
use crate::BackendPool;
#[cfg(feature = "pg")]
use kevy_embedded::Store;

mod admin;
mod lifecycle;
pub mod suppression;

pub use admin::*;
pub use lifecycle::*;

/// Lifecycle status of a queued message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueStatus {
    /// Awaiting first delivery attempt.
    Pending,
    /// Currently being delivered by a worker.
    InFlight,
    /// Successfully accepted by the remote MX.
    Delivered,
    /// Last attempt failed; will retry per backoff schedule.
    Failed,
    /// Permanent failure; will NOT retry. A DSN has been queued back.
    Bounced,
}

impl QueueStatus {
    /// Lower-snake-case rendering for SQL persistence.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InFlight => "inflight",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
            Self::Bounced => "bounced",
        }
    }

    /// Inverse of [`Self::as_str`]; `None` on unknown values.
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

/// One queued outbound message — the full row stored in the outbound queue.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    /// Store-native primary key.
    pub id: i64,
    /// Envelope sender (reverse path).
    pub sender: String,
    /// Envelope recipient (single forward path; multi-recipient messages
    /// fan out into one row per recipient).
    pub recipient: String,
    /// Recipient's domain — extracted for MX-grouped batching.
    pub domain: String,
    /// Full RFC 5322 message body (including headers).
    pub message_data: Vec<u8>,
    /// Current lifecycle status.
    pub status: QueueStatus,
    /// Number of delivery attempts made so far.
    pub attempts: u32,
    /// Cap after which `Failed` flips to `Bounced`.
    pub max_attempts: u32,
    /// Epoch seconds — the earliest time the next attempt is eligible.
    pub next_retry: i64,
    /// Last error response from the remote MX, if any.
    pub last_error: Option<String>,
    /// `Message-ID:` header value, for log correlation.
    pub message_id: Option<String>,
    /// Epoch seconds when the row was first enqueued.
    pub created_at: i64,
    /// Epoch seconds of the most recent update.
    pub updated_at: i64,
    /// `true` when the message came from a forwarding rule rather than a
    /// local sender.
    pub is_forwarded: bool,
}

/// enqueue a message for outbound delivery
#[cfg(feature = "pg")]
pub async fn enqueue(
    pool: &BackendPool,
    sender: &str,
    recipient: &str,
    domain: &str,
    message_data: &[u8],
    message_id: Option<&str>,
    now: i64,
) -> Result<i64, sqlx::Error> {
    enqueue_ex(
        pool,
        sender,
        recipient,
        domain,
        message_data,
        message_id,
        now,
        false,
    )
    .await
}

/// enqueue a message for outbound delivery with forwarding flag
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "pg")]
pub async fn enqueue_ex(
    pool: &BackendPool,
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

/// notify the delivery worker that new messages are queued via the
/// in-process kevy `queue:notify` channel.
#[cfg(feature = "pg")]
pub fn notify(kevy: &Store) {
    let _ = kevy.publish(b"queue:notify", b"1");
}

/// enqueue a message for scheduled delivery at a future time
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "pg")]
pub async fn enqueue_scheduled(
    pool: &BackendPool,
    sender: &str,
    recipient: &str,
    domain: &str,
    message_data: &[u8],
    message_id: Option<&str>,
    created_at: i64,
    scheduled_at: i64,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO outbound_queue (sender, recipient, domain, message_data, status, next_retry, message_id, created_at, updated_at, is_forwarded)
         VALUES ($1, $2, $3, $4, 'pending', $5, $6, $7, $7, false)
         RETURNING id",
    )
    .bind(sender)
    .bind(recipient)
    .bind(domain)
    .bind(message_data)
    .bind(scheduled_at)
    .bind(message_id)
    .bind(created_at)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}
