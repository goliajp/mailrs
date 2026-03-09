use rand_core::{OsRng, RngCore};
use sqlx::PgPool;

use super::{OutboxEntry, Subscription};

/// retry delay schedule in seconds, indexed by attempt number
/// matches outbound_queue pattern: exponential backoff capped at 6 hours
const RETRY_DELAYS: [i64; 8] = [60, 120, 300, 600, 1800, 3600, 7200, 21600];

/// generate a cryptographically random signing secret (64 hex chars = 32 bytes)
pub fn generate_signing_secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// compute retry delay in seconds for a given attempt number
pub fn retry_delay_secs(attempt: u32) -> i64 {
    let idx = (attempt as usize).min(RETRY_DELAYS.len() - 1);
    RETRY_DELAYS[idx]
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
mod tests {
    use super::*;

    #[test]
    fn retry_delay_secs_returns_correct_values() {
        assert_eq!(retry_delay_secs(0), 60);
        assert_eq!(retry_delay_secs(1), 120);
        assert_eq!(retry_delay_secs(2), 300);
        assert_eq!(retry_delay_secs(3), 600);
        assert_eq!(retry_delay_secs(4), 1800);
        assert_eq!(retry_delay_secs(5), 3600);
        assert_eq!(retry_delay_secs(6), 7200);
        assert_eq!(retry_delay_secs(7), 21600);
        // beyond max index caps at last value
        assert_eq!(retry_delay_secs(8), 21600);
        assert_eq!(retry_delay_secs(100), 21600);
    }

    #[test]
    fn generate_signing_secret_produces_64_char_hex() {
        let secret = generate_signing_secret();
        assert_eq!(secret.len(), 64);
        assert!(secret.chars().all(|c| c.is_ascii_hexdigit()));

        // two calls produce different values
        let secret2 = generate_signing_secret();
        assert_ne!(secret, secret2);
    }

    #[test]
    fn webhook_payload_serialization_roundtrip() {
        use crate::webhook::{WebhookData, WebhookPayload};

        let payload = WebhookPayload {
            event: "new_message".to_string(),
            timestamp: "2026-03-10T12:00:00Z".to_string(),
            data: WebhookData {
                user: "user@golia.jp".to_string(),
                thread_id: "abc123".to_string(),
                sender: "someone@example.com".to_string(),
                subject: "Hello".to_string(),
                snippet: "First 100 chars...".to_string(),
            },
        };

        let json = serde_json::to_string(&payload).unwrap();
        let deserialized: WebhookPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload, deserialized);

        // verify key fields are present in json
        assert!(json.contains("\"event\":\"new_message\""));
        assert!(json.contains("\"thread_id\":\"abc123\""));
    }
}
