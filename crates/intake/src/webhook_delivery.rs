//! Webhooks for arriving mail: queued at ingest, POSTed by this loop.
//!
//! The queue is `core_sidestate::families::webhook_outbox` on the network
//! kevy, so whichever core is running drains it.
//!
//! Claiming is exclusive (a `zrem` that either removes the member or does
//! not), so running more than one of these does not double-POST. The loop
//! backs off when the queue is empty —
//! an idle mailbox should not cost a request every five seconds forever.

use std::sync::Arc;
use std::time::Duration;

use mailrs_core_sidestate::families::{webhook_events, webhook_outbox, webhooks};

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

/// A way to reach the network kevy; `None` when it is not configured or
/// cannot be reached.
pub type NetConn = Arc<dyn Fn() -> Option<kevy_client::Connection> + Send + Sync>;

/// Queue the webhook event for a message now stored: `mail.held` when it
/// was held as fraud, `mail.sent` when the user wrote it, `mail.received`
/// otherwise. After the store, so a held message is not announced as new
/// mail and a subscriber that reads the message on the event finds it.
pub fn enqueue_for_arrival(
    net: Option<kevy_client::Connection>,
    user: &str,
    a: &crate::arrival::Arrival<'_>,
    held: bool,
) {
    use webhook_events::kinds as ev;
    let Some(mut conn) = net else {
        return;
    };
    let (kind, sender, data) = match (held, a.is_own) {
        (true, _) => (ev::MAIL_HELD, Some(a.from.as_str()), received_data(a)),
        (false, true) => (
            ev::MAIL_SENT,
            None,
            serde_json::json!({
                "message_id": a.message_id,
                "subject": a.subject,
                "recipients": a.to,
            }),
        ),
        (false, false) => (ev::MAIL_RECEIVED, Some(a.from.as_str()), received_data(a)),
    };
    let event = webhook_events::Event {
        kind,
        user,
        sender,
        thread_id: Some(&a.thread_id),
        data,
    };
    webhook_events::enqueue_logged(&mut conn, &event, now_secs());
}

/// What a received message's event says; `snippet` stays empty, as it
/// always has, so no body text leaves in a webhook.
fn received_data(a: &crate::arrival::Arrival<'_>) -> serde_json::Value {
    serde_json::json!({
        "message_id": a.message_id,
        "subject": a.subject,
        "snippet": "",
    })
}

/// Poll and deliver until the process ends.
pub async fn run(net: NetConn) {
    // a subscriber names the URL, so every delivery goes through the
    // egress check
    let client = mailrs_egress::Egress::new("mailrs-webhook/1.0", Duration::from_secs(10));

    // Said once at boot, because the loop is otherwise silent while the
    // queue is empty — indistinguishable from one that never started, which
    // is the shape of silence this whole pass is about.
    tracing::info!("webhook delivery started");
    let mut idle_rounds = 0u32;
    loop {
        let delivered = drain_once(&net, &client).await;
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
async fn drain_once(net: &NetConn, client: &mailrs_egress::Egress) -> usize {
    let Some(mut conn) = net() else {
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
        let net = net.clone();
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            deliver_one(&net, &client, entry).await;
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
    net: &NetConn,
    client: &mailrs_egress::Egress,
    entry: webhook_outbox::OutboxEntry,
) {
    let Some(mut conn) = net() else {
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

    // no redirects: the signature is for the URL the subscriber registered
    let result = client
        .send(&sub.url, 0, |c, url| {
            c.post(url)
                .header("Content-Type", "application/json")
                .header(
                    "X-Mailrs-Signature",
                    mailrs_webhook_signature::format_header(&signature),
                )
                .header("X-Mailrs-Event", &event)
                .header("X-Mailrs-Delivery", entry.id.to_string())
                .body(entry.payload.clone())
        })
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

#[cfg(test)]
mod event_tests {
    use super::*;
    use crate::arrival::{Threads, derive};
    use mailrs_core_sidestate::families::webhook_events::kinds as ev;

    struct NoThreads;
    impl Threads for NoThreads {
        fn thread_for_message_id(&self, _: &str, _: &str) -> Option<String> {
            None
        }
        fn thread_subject(&self, _: &str, _: &str) -> Option<String> {
            None
        }
    }

    const USER: &str = "alice@example.test";
    const FROM_OUTSIDE: &[u8] = b"From: Carol <carol@partner.test>\r\n\
To: alice@example.test\r\n\
Subject: Lunch\r\n\
Message-ID: <in1@partner.test>\r\n\
\r\n\
Friday?\r\n";
    const FROM_ALICE: &[u8] = b"From: Alice <alice@example.test>\r\n\
To: carol@partner.test\r\n\
Subject: Re: Lunch\r\n\
Message-ID: <out1@example.test>\r\n\
\r\n\
Yes.\r\n";

    /// Subscribe `USER` to `kind` in a fresh store, raise the arrival, and
    /// return what was queued.
    fn raised(name: &str, kind: &str, raw: &[u8], held: bool) -> Vec<serde_json::Value> {
        let url = format!("mem://intake-webhook-{name}");
        let mut c = kevy_client::Connection::connect(&url).expect("kevy");
        webhooks::create(
            &mut c,
            webhooks::NewWebhook {
                account_address: USER.into(),
                url: "https://hooks.example/in".into(),
                event_type: kind.into(),
                filter_sender: None,
                filter_thread_id: None,
            },
        )
        .expect("subscribe");
        let a = derive(&NoThreads, None, USER, "f1", raw, "INBOX");
        enqueue_for_arrival(kevy_client::Connection::connect(&url).ok(), USER, &a, held);
        webhook_outbox::claim_due(&mut c, i64::MAX, 10)
            .expect("claim")
            .into_iter()
            .map(|e| serde_json::from_str(&e.payload).expect("json"))
            .collect()
    }

    #[test]
    fn mail_from_outside_is_received() {
        let got = raised("received", ev::MAIL_RECEIVED, FROM_OUTSIDE, false);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["event"], ev::MAIL_RECEIVED);
        assert_eq!(got[0]["data"]["message_id"], "in1@partner.test");
        assert_eq!(got[0]["data"]["subject"], "Lunch");
        assert_eq!(got[0]["data"]["snippet"], "", "no body text in a webhook");
    }

    /// Held mail is announced as held, and not as new mail.
    #[test]
    fn mail_held_as_fraud_is_held_not_received() {
        let got = raised("held", ev::MAIL_HELD, FROM_OUTSIDE, true);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["event"], ev::MAIL_HELD);
        assert!(raised("held-not-new", ev::MAIL_RECEIVED, FROM_OUTSIDE, true).is_empty());
    }

    /// The user's own sent copy is sent mail, not new mail.
    #[test]
    fn the_users_own_message_is_sent_not_received() {
        let got = raised("sent", ev::MAIL_SENT, FROM_ALICE, false);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["event"], ev::MAIL_SENT);
        assert_eq!(got[0]["data"]["recipients"], "carol@partner.test");
        assert!(got[0]["data"].get("sender").is_none());
        assert!(raised("sent-not-new", ev::MAIL_RECEIVED, FROM_ALICE, false).is_empty());
    }
}
