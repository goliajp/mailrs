use sqlx::PgPool;
use tokio::sync::watch;
use tracing;

use super::{Subscription, WebhookData, WebhookPayload, store};
use crate::event_bus::{EventBus, SmtpEvent};

/// check whether a subscription's filters match the given sender and thread_id
pub(crate) fn matches_subscription(sub: &Subscription, sender: &str, thread_id: &str) -> bool {
    if let Some(ref f) = sub.filter_sender
        && f != sender
    {
        return false;
    }
    if let Some(ref f) = sub.filter_thread_id
        && f != thread_id
    {
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
                    Ok(env) => {
                        if let SmtpEvent::NewMessage { user, thread_id, sender, subject, snippet } = &env.event {
                            match store::find_matching_subscriptions(pool, user, "new_message", sender, thread_id).await {
                                Ok(subs) => {
                                    for sub in subs {
                                        if matches_subscription(&sub, sender, thread_id) {
                                            let payload = build_payload(user, thread_id, sender, subject, snippet);
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
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("webhook listener lagged, missed {n} events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
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
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_sub(filter_sender: Option<&str>, filter_thread_id: Option<&str>) -> Subscription {
        Subscription {
            id: 1,
            account_address: "user@example.com".to_string(),
            url: "https://example.com/hook".to_string(),
            event_type: "new_message".to_string(),
            filter_sender: filter_sender.map(|s| s.to_string()),
            filter_thread_id: filter_thread_id.map(|s| s.to_string()),
            signing_secret: "secret".to_string(),
            active: true,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn no_filter_matches_any_sender_and_thread() {
        let sub = make_sub(None, None);
        assert!(matches_subscription(
            &sub,
            "anyone@example.com",
            "any-thread"
        ));
        assert!(matches_subscription(
            &sub,
            "other@example.com",
            "other-thread"
        ));
    }

    #[test]
    fn filter_sender_matches_only_exact_sender() {
        let sub = make_sub(Some("specific@example.com"), None);
        assert!(matches_subscription(
            &sub,
            "specific@example.com",
            "any-thread"
        ));
        assert!(!matches_subscription(
            &sub,
            "other@example.com",
            "any-thread"
        ));
    }

    #[test]
    fn filter_thread_id_matches_only_exact_thread() {
        let sub = make_sub(None, Some("thread-123"));
        assert!(matches_subscription(
            &sub,
            "anyone@example.com",
            "thread-123"
        ));
        assert!(!matches_subscription(
            &sub,
            "anyone@example.com",
            "thread-456"
        ));
    }

    #[test]
    fn both_filters_require_both_to_match() {
        let sub = make_sub(Some("specific@example.com"), Some("thread-123"));
        assert!(matches_subscription(
            &sub,
            "specific@example.com",
            "thread-123"
        ));
        assert!(!matches_subscription(
            &sub,
            "specific@example.com",
            "thread-456"
        ));
        assert!(!matches_subscription(
            &sub,
            "other@example.com",
            "thread-123"
        ));
        assert!(!matches_subscription(
            &sub,
            "other@example.com",
            "thread-456"
        ));
    }

    #[test]
    fn build_payload_creates_correct_structure() {
        let payload = build_payload("user@ex.com", "t1", "sender@ex.com", "Hello", "Snippet...");
        assert_eq!(payload.event, "new_message");
        assert_eq!(payload.data.user, "user@ex.com");
        assert_eq!(payload.data.thread_id, "t1");
        assert_eq!(payload.data.sender, "sender@ex.com");
        assert_eq!(payload.data.subject, "Hello");
        assert_eq!(payload.data.snippet, "Snippet...");
        // timestamp should be parseable
        assert!(chrono::DateTime::parse_from_rfc3339(&payload.timestamp).is_ok());
    }
}
