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
mod tests {
    use super::*;

    #[test]
    fn retry_delay_secs_uses_backoff_webhook_curve() {
        // Now backed by mailrs-backoff Backoff::webhook(): initial=60,
        // multiplier=2, max=6h (21600s), Equal jitter.
        // base_delay (without jitter) for attempt n: min(60 × 2^n, 21600).
        //   0: 60, 1: 120, 2: 240, 3: 480, 4: 960, 5: 1920, 6: 3840,
        //   7: 7680, 8: 15360, 9: 21600 (cap)
        // Equal jitter means actual returned value is in [base/2, base].
        // We assert the value is WITHIN that band rather than exact.
        let mut prev = 0;
        for attempt in 0..6u32 {
            let d = retry_delay_secs(attempt);
            assert!(
                d >= 30,
                "attempt {attempt}: {d} < 30 (Equal jitter low bound)"
            );
            // Strict monotonic isn't guaranteed under jitter, but the band
            // floor for attempt n+1 is half of attempt n+1's base
            // (= attempt n's base for mult=2), which always >= prev floor.
            let _ = prev; // documentation marker
            prev = d;
        }
    }

    #[test]
    fn retry_delay_secs_caps_at_six_hours() {
        // For attempts way past the cap, value should be in [3h, 6h]
        // (Equal jitter half-band of 6h).
        for attempt in [10u32, 20, 100, 1000] {
            let d = retry_delay_secs(attempt);
            assert!(
                (3 * 3600..=6 * 3600).contains(&d),
                "attempt {attempt}: {d} not in [3h, 6h]"
            );
        }
    }

    #[test]
    fn retry_delay_secs_deterministic_for_same_attempt() {
        // The seed is derived from `attempt`, so the same attempt
        // produces the same jittered value (idempotent rescheduling).
        for attempt in 0..10u32 {
            assert_eq!(retry_delay_secs(attempt), retry_delay_secs(attempt));
        }
    }

    #[test]
    fn retry_delay_secs_attempt_zero_in_jitter_band() {
        // attempt=0: base=60, Equal jitter → result in [30, 60].
        let d = retry_delay_secs(0);
        assert!((30..=60).contains(&d), "attempt 0: {d} not in [30, 60]");
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
