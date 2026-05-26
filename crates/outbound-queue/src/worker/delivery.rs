//! Per-domain delivery: MX resolution, suppression filtering, retry/bounce
//! bookkeeping, DSN enqueueing.

use hickory_resolver::TokioResolver;
use sqlx::PgPool;

use super::smtp::try_deliver_via_mx;
use crate::dsn;
use crate::queue::{self, QueuedMessage};
use crate::retry::{retry_delay_secs, should_bounce};
use crate::{DeliveryEvent, DeliveryEventSender};

/// generate DSN bounce and enqueue it back to the original sender
pub(super) async fn enqueue_dsn(
    pool: &PgPool,
    hostname: &str,
    msg: &QueuedMessage,
    error: &str,
) {
    if msg.sender.is_empty() || msg.sender == "<>" {
        return; // don't bounce bounces
    }
    let dsn_msg = dsn::format_dsn(
        hostname,
        &msg.sender,
        &msg.recipient,
        error,
        msg.message_id.as_deref(),
    );
    let sender_domain = msg
        .sender
        .rsplit_once('@')
        .map(|(_, d)| d)
        .unwrap_or("unknown");
    let now = chrono::Utc::now().timestamp();
    let _ = queue::enqueue(
        pool,
        "<>",
        &msg.sender,
        sender_domain,
        dsn_msg.as_bytes(),
        None,
        now,
    )
    .await;
}

/// deliver messages to a single domain (used by concurrent workers)
#[tracing::instrument(
    name = "outbound.deliver_domain",
    skip(resolver, hostname, messages, pool, event_sender),
    fields(domain, n_messages = messages.len(), max_per_conn),
)]
pub(super) async fn deliver_domain_static(
    resolver: &TokioResolver,
    hostname: &str,
    domain: &str,
    messages: Vec<QueuedMessage>,
    pool: &PgPool,
    max_per_conn: usize,
    event_sender: Option<&DeliveryEventSender>,
) {
    // Wall-clock start for the per-domain delivery batch. Used to
    // emit `mailrs_outbound_delivery_seconds` histogram on each
    // terminal mark (delivered / failed / bounced) — gives ops a
    // distribution of end-to-end delivery latency per outcome.
    let batch_start = std::time::Instant::now();

    // filter out suppressed recipients before delivery
    let mut messages = messages;
    let now_check = chrono::Utc::now().timestamp();
    {
        let mut suppressed_ids = Vec::new();
        for msg in &messages {
            if queue::is_suppressed(pool, &msg.recipient).await {
                tracing::info!("skipping suppressed recipient: {}", msg.recipient);
                let _ = queue::mark_bounced(
                    pool,
                    msg.id,
                    "recipient suppressed (hard bounce history)",
                    now_check,
                )
                .await;
                if let Some(es) = event_sender {
                    es(DeliveryEvent::Bounced {
                        queue_id: msg.id,
                        sender: msg.sender.clone(),
                    });
                }
                suppressed_ids.push(msg.id);
            }
        }
        if !suppressed_ids.is_empty() {
            messages.retain(|msg| !suppressed_ids.contains(&msg.id));
        }
        if messages.is_empty() {
            return;
        }
    }

    // resolve MX records
    let mx_records = match mailrs_smtp_client::resolve_mx(resolver, domain).await {
        Ok(records) => records,
        Err(e) => {
            tracing::warn!("MX resolution failed for {domain}: {e}");
            let now = chrono::Utc::now().timestamp();
            for msg in &messages {
                let delay = retry_delay_secs(msg.attempts);
                if should_bounce(msg.attempts + 1, msg.max_attempts) {
                    let error = format!("MX resolution failed: {e}");
                    let _ = queue::mark_bounced(pool, msg.id, &error, now).await;
                    // record hard bounce for suppression
                    if queue::is_hard_bounce(&error) {
                        let _ = queue::add_suppression(pool, &msg.recipient, &error, None).await;
                    }
                    enqueue_dsn(pool, hostname, msg, &error).await;
                    if let Some(es) = event_sender {
                        es(DeliveryEvent::Bounced {
                            queue_id: msg.id,
                            sender: msg.sender.clone(),
                        });
                    }
                } else {
                    let _ = queue::mark_failed(
                        pool,
                        msg.id,
                        &format!("MX resolution failed: {e}"),
                        now + delay as i64,
                        now,
                    )
                    .await;
                    if let Some(es) = event_sender {
                        es(DeliveryEvent::Failed {
                            queue_id: msg.id,
                            domain: domain.to_string(),
                            error: format!("MX resolution failed: {e}"),
                        });
                    }
                }
            }
            return;
        }
    };

    // split messages into chunks for connection reuse limits
    let chunks: Vec<&[QueuedMessage]> = messages.chunks(max_per_conn).collect();

    // try each MX in priority order
    for mx in &mx_records {
        let mut all_ok = true;
        for chunk in &chunks {
            match try_deliver_via_mx(
                hostname,
                &mx.exchange,
                domain,
                chunk,
                resolver,
                event_sender,
            )
            .await
            {
                Ok(()) => {
                    let now = chrono::Utc::now().timestamp();
                    let elapsed = batch_start.elapsed().as_secs_f64();
                    for msg in *chunk {
                        metrics::histogram!(
                            "mailrs_outbound_delivery_seconds",
                            "outcome" => "delivered",
                        )
                        .record(elapsed);
                        let _ = queue::mark_delivered(pool, msg.id, now).await;
                        if let Some(es) = event_sender {
                            es(DeliveryEvent::Success {
                                queue_id: msg.id,
                                domain: domain.to_string(),
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("delivery to {} via {} failed: {e}", domain, mx.exchange);
                    all_ok = false;
                    break;
                }
            }
        }
        if all_ok {
            tracing::info!(
                "delivered {} messages to {domain} via {}",
                messages.len(),
                mx.exchange
            );
            return;
        }
    }

    // all MX hosts failed — mark remaining undelivered messages
    let now = chrono::Utc::now().timestamp();
    for msg in &messages {
        // skip already delivered messages
        if let Ok(Some(current)) = queue::get_message(pool, msg.id).await
            && current.status == crate::queue::QueueStatus::Delivered
        {
            continue;
        }
        let delay = retry_delay_secs(msg.attempts);
        if should_bounce(msg.attempts + 1, msg.max_attempts) {
            let _ = queue::mark_bounced(pool, msg.id, "all MX hosts failed", now).await;
            // add to suppression if last error was a hard bounce
            if let Some(ref err) = msg.last_error
                && queue::is_hard_bounce(err)
            {
                let _ = queue::add_suppression(pool, &msg.recipient, err, None).await;
            }
            enqueue_dsn(pool, hostname, msg, "all MX hosts failed").await;
            if let Some(es) = event_sender {
                es(DeliveryEvent::Bounced {
                    queue_id: msg.id,
                    sender: msg.sender.clone(),
                });
            }
        } else {
            let _ =
                queue::mark_failed(pool, msg.id, "all MX hosts failed", now + delay as i64, now)
                    .await;
            if let Some(es) = event_sender {
                es(DeliveryEvent::Failed {
                    queue_id: msg.id,
                    domain: domain.to_string(),
                    error: "all MX hosts failed".into(),
                });
            }
        }
    }
}
