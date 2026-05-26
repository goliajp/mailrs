use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{Semaphore, watch};

use crate::event_bus::{EventBus, SmtpEvent};
use crate::system_config::SystemConfigStore;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GlobalWebhookPayload {
    pub event: String,
    pub address: String,
    pub count: u32,
    pub latest: LatestMail,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LatestMail {
    pub from: String,
    pub subject: String,
    pub snippet: String,
    pub timestamp: i64,
}

fn build_payload(
    address: &str,
    sender: &str,
    subject: &str,
    snippet: &str,
) -> GlobalWebhookPayload {
    GlobalWebhookPayload {
        event: "new_mail".to_string(),
        address: address.to_string(),
        count: 1,
        latest: LatestMail {
            from: sender.to_string(),
            subject: subject.to_string(),
            snippet: snippet.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        },
    }
}

/// fire-and-forget global webhook listener (reads URL/key from SystemConfigStore on each event)
pub async fn run(
    event_bus: &EventBus,
    config_store: Arc<SystemConfigStore>,
    mut shutdown: watch::Receiver<bool>,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .expect("failed to build global webhook http client");

    let semaphore = Arc::new(Semaphore::new(10));
    let mut rx = event_bus.subscribe();

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(env) => {
                        if let SmtpEvent::NewMessage { user, sender, subject, snippet, .. } = &env.event {
                            // read url/key from config store each time (runtime-changeable)
                            let cfg = config_store.get();
                            let url = match cfg.webhook_url {
                                Some(ref u) if !u.is_empty() => u.clone(),
                                _ => continue, // no webhook configured, skip
                            };
                            let api_key = cfg.webhook_api_key.clone();

                            let permit = match semaphore.clone().try_acquire_owned() {
                                Ok(p) => p,
                                Err(_) => {
                                    tracing::warn!("global webhook concurrency limit reached, dropping event for {user}");
                                    continue;
                                }
                            };
                            let payload = build_payload(user, sender, subject, snippet);
                            let client = client.clone();
                            tokio::spawn(async move {
                                send_webhook(&client, &url, api_key.as_deref(), &payload).await;
                                drop(permit);
                            });
                        }
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("global webhook listener lagged, missed {n} events");
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

async fn send_webhook(
    client: &reqwest::Client,
    url: &str,
    api_key: Option<&str>,
    payload: &GlobalWebhookPayload,
) {
    let mut req = client.post(url).json(payload).header("X-Caller", "mailrs");
    if let Some(key) = api_key {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::debug!("global webhook delivered to {url}: {}", resp.status());
        }
        Ok(resp) => {
            tracing::warn!(
                "global webhook to {url} returned non-success: {}",
                resp.status()
            );
        }
        Err(e) => {
            tracing::warn!("global webhook to {url} failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_serializes_to_expected_format() {
        let payload = build_payload(
            "lihao@golia.jp",
            "sender@example.com",
            "Hello",
            "First 100 chars",
        );
        let json = serde_json::to_value(&payload).unwrap();

        assert_eq!(json["event"], "new_mail");
        assert_eq!(json["address"], "lihao@golia.jp");
        assert_eq!(json["count"], 1);
        assert_eq!(json["latest"]["from"], "sender@example.com");
        assert_eq!(json["latest"]["subject"], "Hello");
        assert_eq!(json["latest"]["snippet"], "First 100 chars");
        assert!(json["latest"]["timestamp"].is_number());
    }

    #[test]
    fn payload_event_is_new_mail() {
        let payload = build_payload("a@b.com", "c@d.com", "Sub", "Snip");
        assert_eq!(payload.event, "new_mail");
        assert_eq!(payload.count, 1);
    }
}
