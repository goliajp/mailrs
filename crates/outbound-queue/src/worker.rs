use std::collections::HashMap;
use std::sync::Arc;

use hickory_resolver::TokioResolver;
use sqlx::PgPool;

use mail_auth::MessageAuthenticator;

use crate::dkim_sign::{self, DkimSignConfig};
use crate::dsn;
use crate::queue::{self, QueuedMessage};
use crate::retry::{retry_delay_secs, should_bounce};
use crate::{DeliveryEvent, DeliveryEventSender};

/// delivery worker configuration
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub poll_interval_secs: u64,
    pub batch_size: u32,
    pub max_attempts: u32,
    pub max_concurrent_domains: usize,
    pub max_messages_per_connection: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 30,
            batch_size: 50,
            max_attempts: 8,
            max_concurrent_domains: 8,
            max_messages_per_connection: 50,
        }
    }
}

/// group queued messages by target domain for efficient delivery
pub fn group_by_domain(messages: Vec<QueuedMessage>) -> HashMap<String, Vec<QueuedMessage>> {
    let mut groups: HashMap<String, Vec<QueuedMessage>> = HashMap::new();
    for msg in messages {
        groups.entry(msg.domain.clone()).or_default().push(msg);
    }
    groups
}

/// background delivery worker that polls the queue and delivers messages
pub struct DeliveryWorker {
    config: WorkerConfig,
    pool: PgPool,
    resolver: TokioResolver,
    hostname: String,
    dkim: Option<DkimSignConfig>,
    authenticator: Option<MessageAuthenticator>,
    event_sender: Option<DeliveryEventSender>,
    valkey_url: Option<String>,
}

impl DeliveryWorker {
    pub fn new(
        config: WorkerConfig,
        pool: PgPool,
        resolver: TokioResolver,
        hostname: String,
    ) -> Self {
        // create authenticator for ARC sealing (non-fatal if fails)
        let authenticator = MessageAuthenticator::new_system_conf()
            .map_err(|e| tracing::warn!("failed to create authenticator for ARC: {e}"))
            .ok();

        Self {
            config,
            pool,
            resolver,
            hostname,
            dkim: None,
            authenticator,
            event_sender: None,
            valkey_url: None,
        }
    }

    pub fn with_dkim(mut self, dkim: DkimSignConfig) -> Self {
        self.dkim = Some(dkim);
        self
    }

    pub fn with_event_sender(mut self, sender: DeliveryEventSender) -> Self {
        self.event_sender = Some(sender);
        self
    }

    pub fn with_valkey(mut self, url: String) -> Self {
        self.valkey_url = Some(url);
        self
    }

    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        tracing::info!("delivery worker started (poll_interval={}s)", self.config.poll_interval_secs);

        // try to subscribe to Valkey queue:notify for fast wakeup
        let mut notify_rx = self.spawn_valkey_listener();

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(self.config.poll_interval_secs)) => {}
                _ = wait_for_notify(&mut notify_rx) => {}
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("delivery worker shutting down");
                        return;
                    }
                }
            }

            if let Err(e) = self.poll_and_deliver().await {
                tracing::error!("delivery worker error: {e}");
            }
        }
    }

    fn spawn_valkey_listener(&self) -> Option<tokio::sync::mpsc::Receiver<()>> {
        let url = self.valkey_url.as_ref()?;
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let url = url.clone();
        tokio::spawn(async move {
            loop {
                match redis::Client::open(url.as_str()) {
                    Ok(client) => {
                        match client.get_async_pubsub().await {
                            Ok(mut pubsub) => {
                                if pubsub.subscribe("queue:notify").await.is_err() {
                                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                    continue;
                                }
                                tracing::info!("delivery worker subscribed to queue:notify");
                                use futures_util::StreamExt;
                                let mut stream = pubsub.on_message();
                                loop {
                                    match stream.next().await {
                                        Some(_msg) => {
                                            let _ = tx.try_send(());
                                        }
                                        None => break,
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("valkey pubsub connect failed: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("valkey client create failed: {e}");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
        Some(rx)
    }

    async fn poll_and_deliver(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let now = chrono::Utc::now().timestamp();
        let messages = queue::dequeue(&self.pool, now, self.config.batch_size).await?;

        if messages.is_empty() {
            return Ok(());
        }

        tracing::info!("dequeued {} messages for delivery", messages.len());

        // mark all as inflight
        for msg in &messages {
            let _ = queue::mark_inflight(&self.pool, msg.id, now).await;
        }

        // apply ARC sealing (for forwarded messages) + DKIM signing
        let messages: Vec<QueuedMessage> = if let Some(ref dkim) = self.dkim {
            let mut signed_msgs = Vec::with_capacity(messages.len());
            for mut msg in messages {
                // ARC seal forwarded messages before DKIM signing
                if msg.is_forwarded {
                    if let Some(ref auth) = self.authenticator {
                        match dkim_sign::arc_seal_message(dkim, auth, &msg.message_data).await {
                            Ok(sealed) => msg.message_data = sealed,
                            Err(e) => tracing::warn!("ARC sealing failed for msg {}: {e}", msg.id),
                        }
                    }
                }
                // DKIM sign
                match dkim.sign(&msg.message_data) {
                    Ok(signed) => msg.message_data = signed,
                    Err(e) => tracing::warn!("DKIM signing failed for msg {}: {e}", msg.id),
                }
                signed_msgs.push(msg);
            }
            signed_msgs
        } else {
            messages
        };

        let groups = group_by_domain(messages);
        let pool = self.pool.clone();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_concurrent_domains));

        let mut handles = Vec::new();
        for (domain, domain_messages) in groups {
            let sem = semaphore.clone();
            let pool = pool.clone();
            let resolver = self.resolver.clone();
            let hostname = self.hostname.clone();
            let max_per_conn = self.config.max_messages_per_connection;
            let event_sender = self.event_sender.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                deliver_domain_static(&resolver, &hostname, &domain, domain_messages, &pool, max_per_conn, event_sender.as_ref()).await;
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

}

/// wait for a Valkey notify signal, or never resolve if no listener
async fn wait_for_notify(rx: &mut Option<tokio::sync::mpsc::Receiver<()>>) {
    match rx {
        Some(ref mut r) => { r.recv().await; }
        None => std::future::pending().await,
    }
}

/// generate DSN bounce and enqueue it back to the original sender
async fn enqueue_dsn(pool: &PgPool, hostname: &str, msg: &QueuedMessage, error: &str) {
    if msg.sender.is_empty() || msg.sender == "<>" {
        return; // don't bounce bounces
    }
    let dsn_msg = dsn::format_dsn(hostname, &msg.sender, &msg.recipient, error, msg.message_id.as_deref());
    let sender_domain = msg.sender.rsplit_once('@').map(|(_, d)| d).unwrap_or("unknown");
    let now = chrono::Utc::now().timestamp();
    let _ = queue::enqueue(pool, "<>", &msg.sender, sender_domain, dsn_msg.as_bytes(), None, now).await;
}

/// deliver messages to a single domain (used by concurrent workers)
async fn deliver_domain_static(
    resolver: &TokioResolver,
    hostname: &str,
    domain: &str,
    messages: Vec<QueuedMessage>,
    pool: &PgPool,
    max_per_conn: usize,
    event_sender: Option<&DeliveryEventSender>,
) {
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
                    enqueue_dsn(pool, hostname, msg, &error).await;
                    if let Some(es) = event_sender {
                        es(DeliveryEvent::Bounced { queue_id: msg.id, sender: msg.sender.clone() });
                    }
                } else {
                    let _ = queue::mark_failed(
                        pool,
                        msg.id,
                        &format!("MX resolution failed: {e}"),
                        now + delay as i64,
                        now,
                    ).await;
                    if let Some(es) = event_sender {
                        es(DeliveryEvent::Failed { queue_id: msg.id, domain: domain.to_string(), error: format!("MX resolution failed: {e}") });
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
            match try_deliver_via_mx(hostname, &mx.exchange, chunk).await {
                Ok(()) => {
                    let now = chrono::Utc::now().timestamp();
                    for msg in *chunk {
                        let _ = queue::mark_delivered(pool, msg.id, now).await;
                        if let Some(es) = event_sender {
                            es(DeliveryEvent::Success { queue_id: msg.id, domain: domain.to_string() });
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
            tracing::info!("delivered {} messages to {domain} via {}", messages.len(), mx.exchange);
            return;
        }
    }

    // all MX hosts failed — mark remaining undelivered messages
    let now = chrono::Utc::now().timestamp();
    for msg in &messages {
        // skip already delivered messages
        if let Ok(Some(current)) = queue::get_message(pool, msg.id).await {
            if current.status == crate::queue::QueueStatus::Delivered {
                continue;
            }
        }
        let delay = retry_delay_secs(msg.attempts);
        if should_bounce(msg.attempts + 1, msg.max_attempts) {
            let _ = queue::mark_bounced(pool, msg.id, "all MX hosts failed", now).await;
            enqueue_dsn(pool, hostname, msg, "all MX hosts failed").await;
            if let Some(es) = event_sender {
                es(DeliveryEvent::Bounced { queue_id: msg.id, sender: msg.sender.clone() });
            }
        } else {
            let _ = queue::mark_failed(
                pool,
                msg.id,
                "all MX hosts failed",
                now + delay as i64,
                now,
            ).await;
            if let Some(es) = event_sender {
                es(DeliveryEvent::Failed { queue_id: msg.id, domain: domain.to_string(), error: "all MX hosts failed".into() });
            }
        }
    }
}

/// try to deliver messages via a specific MX host
async fn try_deliver_via_mx(
    hostname: &str,
    mx_host: &str,
    messages: &[QueuedMessage],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut smtp = mailrs_smtp_client::SmtpConnection::connect(mx_host, 25).await?;
    let ehlo_resp = smtp.ehlo(hostname).await?;

    if !ehlo_resp.is_positive() {
        return Err(format!("EHLO rejected: {}", ehlo_resp.message()).into());
    }

    // try STARTTLS if available (starttls consumes self, reconnect on failure)
    if ehlo_resp.message().contains("STARTTLS") {
        match smtp.starttls(mx_host).await {
            Ok(tls_smtp) => {
                smtp = tls_smtp;
                let _ = smtp.ehlo(hostname).await?;
            }
            Err(e) => {
                tracing::warn!("STARTTLS failed for {mx_host}: {e}, reconnecting without TLS");
                smtp = mailrs_smtp_client::SmtpConnection::connect(mx_host, 25).await?;
                let resp = smtp.ehlo(hostname).await?;
                if !resp.is_positive() {
                    return Err(format!("EHLO rejected on reconnect: {}", resp.message()).into());
                }
            }
        }
    }

    for msg in messages {
        let to = [msg.recipient.as_str()];
        let resp = smtp.deliver(&msg.sender, &to, &msg.message_data).await?;
        if !resp.is_positive() {
            return Err(format!("delivery failed: {}", resp.message()).into());
        }
    }

    let _ = smtp.quit().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::QueueStatus;

    fn make_msg(id: i64, domain: &str) -> QueuedMessage {
        QueuedMessage {
            id,
            sender: "sender@example.com".into(),
            recipient: format!("rcpt@{domain}"),
            domain: domain.into(),
            message_data: vec![],
            status: QueueStatus::Pending,
            attempts: 0,
            max_attempts: 8,
            next_retry: 0,
            last_error: None,
            message_id: None,
            created_at: 0,
            updated_at: 0,
            is_forwarded: false,
        }
    }

    #[test]
    fn group_by_domain_groups() {
        let messages = vec![
            make_msg(1, "a.com"),
            make_msg(2, "b.com"),
            make_msg(3, "a.com"),
        ];
        let groups = group_by_domain(messages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["a.com"].len(), 2);
        assert_eq!(groups["b.com"].len(), 1);
    }

    #[test]
    fn group_by_domain_empty() {
        let groups = group_by_domain(vec![]);
        assert!(groups.is_empty());
    }

    #[test]
    fn delivery_worker_config_defaults() {
        let cfg = WorkerConfig::default();
        assert_eq!(cfg.poll_interval_secs, 30);
        assert_eq!(cfg.batch_size, 50);
        assert_eq!(cfg.max_attempts, 8);
        assert_eq!(cfg.max_concurrent_domains, 8);
        assert_eq!(cfg.max_messages_per_connection, 50);
    }
}
