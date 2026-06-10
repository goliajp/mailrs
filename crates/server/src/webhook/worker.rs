use std::time::Duration;

use crate::pg::BackendPool;
use tokio::sync::watch;
use tracing;

use super::{OutboxEntry, signer, store};

/// background worker that polls the webhook outbox and delivers payloads
pub struct WebhookWorker {
    pool: BackendPool,
    client: reqwest::Client,
    poll_interval: Duration,
    batch_size: i32,
}

impl WebhookWorker {
    pub fn new(pool: BackendPool) -> Self {
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
        // Atomic SKIP LOCKED claim: one statement, single fsync,
        // multi-worker safe (no duplicate POSTs). Replaces the prior
        // dequeue + per-entry mark_inflight flow.
        let entries = match store::claim_for_delivery(&self.pool, now, self.batch_size).await {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("webhook worker: failed to claim: {e}");
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

/// build the headers and deliver a single outbox entry. The entry
/// has already been atomically transitioned to `inflight` by the
/// poll-loop's `claim_for_delivery`, so this function does not
/// re-mark it.
async fn deliver_one(client: &reqwest::Client, pool: &BackendPool, entry: OutboxEntry) {
    let now = chrono::Utc::now().timestamp();
    let attempt = entry.attempts + 1;

    // load subscription to get url and signing_secret
    let sub = match store::get_subscription(pool, entry.subscription_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            let _ = store::mark_failed(
                pool,
                entry.id,
                "subscription not found",
                attempt,
                entry.max_attempts,
                now,
            )
            .await;
            return;
        }
        Err(e) => {
            let _ = store::mark_failed(
                pool,
                entry.id,
                &format!("db error: {e}"),
                attempt,
                entry.max_attempts,
                now,
            )
            .await;
            return;
        }
    };

    // serialize payload
    let payload_bytes = serde_json::to_vec(&entry.payload).unwrap_or_default();

    // sign
    let signature = signer::sign_payload(sub.signing_secret.as_bytes(), &payload_bytes);
    let sig_header = signer::format_signature_header(&signature);

    // extract event type from payload
    let event_type = entry
        .payload
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
            if let Err(e) =
                store::mark_failed(pool, entry.id, &error, attempt, entry.max_attempts, now).await
            {
                tracing::error!("webhook: failed to mark failed {}: {e}", entry.id);
            }
        }
        Err(e) => {
            let error = e.to_string();
            if let Err(e) =
                store::mark_failed(pool, entry.id, &error, attempt, entry.max_attempts, now).await
            {
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
mod tests {
    use super::*;

    #[test]
    fn build_headers_includes_all_required_headers() {
        let payload = br#"{"event":"new_message"}"#;
        let headers = build_headers("my-secret", payload, "new_message", 42);

        assert_eq!(headers.len(), 4);

        let names: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"Content-Type"));
        assert!(names.contains(&"X-Mailrs-Signature"));
        assert!(names.contains(&"X-Mailrs-Event"));
        assert!(names.contains(&"X-Mailrs-Delivery"));
    }

    #[test]
    fn build_headers_content_type_is_json() {
        let headers = build_headers("s", b"p", "new_message", 1);
        let ct = headers.iter().find(|(k, _)| k == "Content-Type").unwrap();
        assert_eq!(ct.1, "application/json");
    }

    #[test]
    fn build_headers_signature_has_sha256_prefix() {
        let headers = build_headers("secret", b"payload", "new_message", 1);
        let sig = headers
            .iter()
            .find(|(k, _)| k == "X-Mailrs-Signature")
            .unwrap();
        assert!(sig.1.starts_with("sha256="));
    }

    #[test]
    fn build_headers_event_matches_input() {
        let headers = build_headers("s", b"p", "new_message", 1);
        let evt = headers.iter().find(|(k, _)| k == "X-Mailrs-Event").unwrap();
        assert_eq!(evt.1, "new_message");
    }

    #[test]
    fn build_headers_delivery_id_matches() {
        let headers = build_headers("s", b"p", "new_message", 999);
        let del = headers
            .iter()
            .find(|(k, _)| k == "X-Mailrs-Delivery")
            .unwrap();
        assert_eq!(del.1, "999");
    }

    #[test]
    fn build_headers_signature_is_deterministic() {
        let h1 = build_headers("secret", b"payload", "new_message", 1);
        let h2 = build_headers("secret", b"payload", "new_message", 1);
        let sig1 = &h1
            .iter()
            .find(|(k, _)| k == "X-Mailrs-Signature")
            .unwrap()
            .1;
        let sig2 = &h2
            .iter()
            .find(|(k, _)| k == "X-Mailrs-Signature")
            .unwrap()
            .1;
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn build_headers_different_secrets_produce_different_signatures() {
        let h1 = build_headers("secret1", b"payload", "new_message", 1);
        let h2 = build_headers("secret2", b"payload", "new_message", 1);
        let sig1 = &h1
            .iter()
            .find(|(k, _)| k == "X-Mailrs-Signature")
            .unwrap()
            .1;
        let sig2 = &h2
            .iter()
            .find(|(k, _)| k == "X-Mailrs-Signature")
            .unwrap()
            .1;
        assert_ne!(sig1, sig2);
    }
}
