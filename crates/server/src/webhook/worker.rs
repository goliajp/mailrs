use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::watch;
use tracing;

use super::{signer, store, OutboxEntry};

/// background worker that polls the webhook outbox and delivers payloads
pub struct WebhookWorker {
    pool: PgPool,
    client: reqwest::Client,
    poll_interval: Duration,
    batch_size: i32,
}

impl WebhookWorker {
    pub fn new(pool: PgPool) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("failed to build reqwest client");

        Self {
            pool,
            client,
            poll_interval: Duration::from_secs(5),
            batch_size: 50,
        }
    }

    /// run the delivery loop until shutdown is signalled
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(self.poll_interval) => {
                    self.poll_and_deliver().await;
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        return;
                    }
                }
            }
        }
    }

    async fn poll_and_deliver(&self) {
        let now = chrono::Utc::now().timestamp();
        let entries = match store::dequeue_pending(&self.pool, now, self.batch_size).await {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("webhook worker: failed to dequeue: {e}");
                return;
            }
        };

        if entries.is_empty() {
            return;
        }

        // use a semaphore to limit concurrency to 10
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
        let mut handles = Vec::new();

        for entry in entries {
            let permit = semaphore.clone().acquire_owned().await;
            let pool = self.pool.clone();
            let client = self.client.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                deliver_one(&client, &pool, entry).await;
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }
    }
}

/// build the headers and deliver a single outbox entry
async fn deliver_one(client: &reqwest::Client, pool: &PgPool, entry: OutboxEntry) {
    let now = chrono::Utc::now().timestamp();
    let attempt = entry.attempts + 1;

    // mark inflight
    if let Err(e) = store::mark_inflight(pool, entry.id, now).await {
        tracing::error!("webhook: failed to mark inflight {}: {e}", entry.id);
        return;
    }

    // load subscription to get url and signing_secret
    let sub = match store::get_subscription(pool, entry.subscription_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            let _ = store::mark_failed(
                pool, entry.id, "subscription not found", attempt, entry.max_attempts, now,
            ).await;
            return;
        }
        Err(e) => {
            let _ = store::mark_failed(
                pool, entry.id, &format!("db error: {e}"), attempt, entry.max_attempts, now,
            ).await;
            return;
        }
    };

    // serialize payload
    let payload_bytes = serde_json::to_vec(&entry.payload).unwrap_or_default();

    // sign
    let signature = signer::sign_payload(sub.signing_secret.as_bytes(), &payload_bytes);
    let sig_header = signer::format_signature_header(&signature);

    // extract event type from payload
    let event_type = entry.payload
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // deliver
    let result = client
        .post(&sub.url)
        .header("Content-Type", "application/json")
        .header("X-Mailrs-Signature", &sig_header)
        .header("X-Mailrs-Event", &event_type)
        .header("X-Mailrs-Delivery", entry.id.to_string())
        .body(payload_bytes)
        .send()
        .await;

    let now = chrono::Utc::now().timestamp();

    match result {
        Ok(resp) if resp.status().is_success() => {
            if let Err(e) = store::mark_delivered(pool, entry.id, now).await {
                tracing::error!("webhook: failed to mark delivered {}: {e}", entry.id);
            }
        }
        Ok(resp) => {
            let error = format!("HTTP {}", resp.status().as_u16());
            if let Err(e) = store::mark_failed(pool, entry.id, &error, attempt, entry.max_attempts, now).await {
                tracing::error!("webhook: failed to mark failed {}: {e}", entry.id);
            }
        }
        Err(e) => {
            let error = e.to_string();
            if let Err(e) = store::mark_failed(pool, entry.id, &error, attempt, entry.max_attempts, now).await {
                tracing::error!("webhook: failed to mark failed {}: {e}", entry.id);
            }
        }
    }
}

/// build webhook delivery headers (exposed for testing)
#[cfg(test)]
pub(crate) fn build_headers(
    signing_secret: &str,
    payload_bytes: &[u8],
    event_type: &str,
    delivery_id: i64,
) -> Vec<(String, String)> {
    let signature = signer::sign_payload(signing_secret.as_bytes(), payload_bytes);
    let sig_header = signer::format_signature_header(&signature);

    vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("X-Mailrs-Signature".to_string(), sig_header),
        ("X-Mailrs-Event".to_string(), event_type.to_string()),
        ("X-Mailrs-Delivery".to_string(), delivery_id.to_string()),
    ]
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
