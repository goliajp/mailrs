//! The webhook delivery loop.
//!
//! A user could create a subscription and nothing would ever fire: the
//! monolith had a worker over 316 lines of SQL and the lane production runs
//! had neither queue nor worker. The queue is
//! `core_sidestate::families::webhook_outbox`; this drains it.
//!
//! Claiming is exclusive (a `zrem` that either removes the member or does
//! not), so running more than one of these does not double-POST. The loop
//! backs off when the queue is empty, per `periodic-work-must-converge` —
//! an idle mailbox should not cost a request every five seconds forever.

use std::sync::Arc;
use std::time::Duration;

use mailrs_core_sidestate::families::{webhook_outbox, webhooks};

use crate::FastcoreState;

/// Delivery attempts running at once.
const CONCURRENCY: usize = 8;
/// Entries taken per pass.
const BATCH: usize = 50;
/// Poll interval when there was something to do.
const BUSY_INTERVAL: Duration = Duration::from_secs(5);
/// Longest interval when there has been nothing for a while.
const IDLE_INTERVAL: Duration = Duration::from_secs(120);

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Poll and deliver until the process ends.
pub async fn spawn(state: Arc<FastcoreState>) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(err = %e, "webhook delivery: no http client, loop not started");
            return;
        }
    };

    // Said once at boot, because the loop is otherwise silent while the
    // queue is empty — indistinguishable from one that never started, which
    // is the shape of silence this whole pass is about.
    tracing::info!("webhook delivery started");
    let mut idle_rounds = 0u32;
    loop {
        let delivered = drain_once(&state, &client).await;
        idle_rounds = match delivered {
            0 => idle_rounds.saturating_add(1),
            _ => 0,
        };
        // Doubling from the busy interval, capped. A queue that just went
        // quiet is still polled promptly; one quiet for an hour is not.
        let wait = crate::idle_backoff::idle_backoff(BUSY_INTERVAL, IDLE_INTERVAL, idle_rounds);
        tokio::time::sleep(wait).await;
    }
}

/// One pass. Returns how many entries were attempted.
async fn drain_once(state: &Arc<FastcoreState>, client: &reqwest::Client) -> usize {
    let Some(mut conn) = state.net_conn() else {
        return 0;
    };
    let now = now_secs();
    let entries = match webhook_outbox::claim_due(&mut conn, now, BATCH) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(err = %e, "webhook delivery: claim failed");
            return 0;
        }
    };
    if entries.is_empty() {
        return 0;
    }
    let attempted = entries.len();

    let semaphore = Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
    let mut handles = Vec::new();
    for entry in entries {
        let permit = semaphore.clone().acquire_owned().await;
        let state = state.clone();
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            deliver_one(&state, &client, entry).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    tracing::info!(attempted, "webhook delivery pass");
    attempted
}

/// Look up the subscription, sign, POST, and record what happened.
async fn deliver_one(
    state: &Arc<FastcoreState>,
    client: &reqwest::Client,
    entry: webhook_outbox::OutboxEntry,
) {
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let subs = webhooks::list(&mut conn, &entry.account_address).unwrap_or_default();
    let Some(sub) = subs.into_iter().find(|s| s.id == entry.subscription_id) else {
        // The subscription was deleted while this was queued. Dropping the
        // entry is right — retrying could not succeed — and it is recorded
        // rather than silently forgotten.
        tracing::info!(
            entry = entry.id,
            subscription = entry.subscription_id,
            "webhook delivery: subscription gone, dropping entry"
        );
        let _ = webhook_outbox::mark_delivered(&mut conn, entry.id);
        return;
    };

    let payload = entry.payload.as_bytes();
    let signature = mailrs_webhook_signature::sign(sub.signing_secret.as_bytes(), payload);
    let event = serde_json::from_str::<serde_json::Value>(&entry.payload)
        .ok()
        .and_then(|v| v.get("event").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_else(|| "unknown".into());

    let result = client
        .post(&sub.url)
        .header("Content-Type", "application/json")
        .header(
            "X-Mailrs-Signature",
            mailrs_webhook_signature::format_header(&signature),
        )
        .header("X-Mailrs-Event", &event)
        .header("X-Mailrs-Delivery", entry.id.to_string())
        .body(entry.payload.clone())
        .send()
        .await;

    let now = now_secs();
    let error = match result {
        Ok(resp) if resp.status().is_success() => {
            if let Err(e) = webhook_outbox::mark_delivered(&mut conn, entry.id) {
                tracing::error!(err = %e, entry = entry.id, "webhook: mark delivered failed");
            }
            return;
        }
        Ok(resp) => format!("HTTP {}", resp.status().as_u16()),
        Err(e) => e.to_string(),
    };

    match webhook_outbox::mark_failed(&mut conn, &entry, &error, now) {
        Ok(webhook_outbox::AfterFailure::DeadLettered) => {
            tracing::warn!(
                entry = entry.id,
                url = %sub.url,
                %error,
                "webhook dead-lettered after {} attempts",
                webhook_outbox::MAX_ATTEMPTS
            );
        }
        Ok(webhook_outbox::AfterFailure::Retry(due)) => {
            tracing::info!(entry = entry.id, due, %error, "webhook delivery failed, will retry");
        }
        Err(e) => tracing::error!(err = %e, entry = entry.id, "webhook: mark failed failed"),
    }
}
