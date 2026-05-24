use sqlx::PgPool;
use tokio::sync::watch;
use tracing;

use crate::event_bus::{EventBus, SmtpEvent};
use super::{store, Subscription, WebhookData, WebhookPayload};

/// check whether a subscription's filters match the given sender and thread_id
pub(crate) fn matches_subscription(sub: &Subscription, sender: &str, thread_id: &str) -> bool {
    if let Some(ref f) = sub.filter_sender
        && f != sender {
            return false;
        }
    if let Some(ref f) = sub.filter_thread_id
        && f != thread_id {
            return false;
        }
    true
}

/// build a webhook payload from event data
fn build_payload(
    user: &str,
    thread_id: &str,
    sender: &str,
    subject: &str,
    snippet: &str,
) -> WebhookPayload {
    WebhookPayload {
        event: "new_message".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        data: WebhookData {
            user: user.to_string(),
            thread_id: thread_id.to_string(),
            sender: sender.to_string(),
            subject: subject.to_string(),
            snippet: snippet.to_string(),
        },
    }
}

/// run the webhook event listener, subscribing to EventBus and enqueuing deliveries
pub async fn run(event_bus: &EventBus, pool: &PgPool, mut shutdown: watch::Receiver<bool>) {
    let mut rx = event_bus.subscribe();

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(SmtpEvent::NewMessage { user, thread_id, sender, subject, snippet }) => {
                        match store::find_matching_subscriptions(pool, &user, "new_message", &sender, &thread_id).await {
                            Ok(subs) => {
                                for sub in subs {
                                    if matches_subscription(&sub, &sender, &thread_id) {
                                        let payload = build_payload(&user, &thread_id, &sender, &subject, &snippet);
                                        let now = chrono::Utc::now().timestamp();
                                        if let Ok(json) = serde_json::to_value(&payload)
                                            && let Err(e) = store::enqueue_delivery(pool, sub.id, &json, now).await {
                                                tracing::error!("failed to enqueue webhook delivery for sub {}: {e}", sub.id);
                                            }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("failed to find matching webhook subscriptions: {e}");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("webhook listener lagged, missed {n} events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                    _ => {}
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "listener_tests.rs"]
mod tests;
