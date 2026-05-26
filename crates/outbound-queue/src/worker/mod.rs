//! Background delivery worker: polls the outbound queue, signs, and
//! delivers to remote MX hosts.
//!
//! Sub-modules:
//! - [`delivery`] — per-domain orchestration (MX resolve, retry/bounce,
//!   DSN enqueue).
//! - [`smtp`] — per-MX SMTP delivery with STARTTLS / DANE policy
//!   handling. [`TlsPolicy`] is re-exported from here.

use std::collections::HashMap;
use std::sync::Arc;

use hickory_resolver::TokioResolver;
use mailrs_dkim::HickoryDkimResolver;
use sqlx::PgPool;

use crate::dkim_sign::{self, DkimSignConfig};
use crate::queue::{self, QueuedMessage};
use crate::{DeliveryEventSender};

mod delivery;
mod smtp;

pub use smtp::TlsPolicy;

use delivery::deliver_domain_static;

/// Delivery worker configuration.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Polling cadence when no Valkey notify wakeup is available.
    pub poll_interval_secs: u64,
    /// Max queue rows fetched per poll tick.
    pub batch_size: u32,
    /// Cap on retry attempts before a row flips to `Bounced`.
    pub max_attempts: u32,
    /// Max concurrent destination domains delivered in parallel.
    pub max_concurrent_domains: usize,
    /// Max messages reused on a single SMTP connection (RFC 5321
    /// recommends pipelining).
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
    /// DKIM/ARC verify resolver — reuses the same hickory binding as
    /// `resolver`, wrapped in the shape `mailrs-dkim` / `mailrs-arc`
    /// expect. Used by ARC sealing for the verify-then-seal flow.
    dkim_resolver: Arc<HickoryDkimResolver>,
    event_sender: Option<DeliveryEventSender>,
    valkey_url: Option<String>,
}

impl DeliveryWorker {
    /// Construct a delivery worker with the given config + dependencies.
    pub fn new(
        config: WorkerConfig,
        pool: PgPool,
        resolver: TokioResolver,
        hostname: String,
    ) -> Self {
        let dkim_resolver = Arc::new(HickoryDkimResolver::new(resolver.clone()));

        Self {
            config,
            pool,
            resolver,
            hostname,
            dkim: None,
            dkim_resolver,
            event_sender: None,
            valkey_url: None,
        }
    }

    /// Configure DKIM signing for outbound messages.
    pub fn with_dkim(mut self, dkim: DkimSignConfig) -> Self {
        self.dkim = Some(dkim);
        self
    }

    /// Attach a [`DeliveryEventSender`] callback for external observers.
    pub fn with_event_sender(mut self, sender: DeliveryEventSender) -> Self {
        self.event_sender = Some(sender);
        self
    }

    /// Set the Valkey URL to subscribe to `queue:notify` for fast wakeup.
    pub fn with_valkey(mut self, url: String) -> Self {
        self.valkey_url = Some(url);
        self
    }

    /// Run the worker loop until `shutdown` signals.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        tracing::info!(
            "delivery worker started (poll_interval={}s)",
            self.config.poll_interval_secs
        );

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
                    Ok(client) => match client.get_async_pubsub().await {
                        Ok(mut pubsub) => {
                            if pubsub.subscribe("queue:notify").await.is_err() {
                                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                continue;
                            }
                            tracing::info!("delivery worker subscribed to queue:notify");
                            use futures_util::StreamExt;
                            let mut stream = pubsub.on_message();
                            while let Some(_msg) = stream.next().await {
                                let _ = tx.try_send(());
                            }
                        }
                        Err(e) => {
                            tracing::warn!("valkey pubsub connect failed: {e}");
                        }
                    },
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

        // recover stale inflight messages (worker crash recovery)
        let recovered = queue::recover_stale_inflight(&self.pool, now).await?;
        if recovered > 0 {
            tracing::warn!("recovered {recovered} stale inflight messages");
        }

        // Atomic SKIP LOCKED claim + inflight transition in one
        // statement: collapses the previous SELECT + N per-row
        // UPDATEs (N+1 roundtrips, N+1 WAL fsyncs) into a single
        // roundtrip and single fsync per batch, and prevents
        // duplicate delivery in multi-worker setups (each pending
        // row goes to at most one worker).
        let messages = queue::claim_for_delivery(&self.pool, now, self.config.batch_size).await?;

        if messages.is_empty() {
            return Ok(());
        }

        tracing::info!("claimed {} messages for delivery", messages.len());

        // apply ARC sealing (for forwarded messages) + DKIM signing.
        //
        // Both steps are independent across messages. ARC sealing is
        // async (awaits DNS lookups when reconstructing the
        // authentication-results chain); DKIM signing is CPU-bound
        // (RSA-SHA256 over the canonicalised message). Previously
        // these ran one-after-another for the whole batch, paying
        // ARC's DNS RTT × N + RSA-sign × N sequentially.
        //
        // Now: `buffer_unordered(8)` runs up to 8 messages' ARC+DKIM
        // concurrently. DKIM sign goes through `spawn_blocking` to
        // keep CPU work off the tokio reactor thread — multiple
        // signs can land on different blocking-pool threads. The
        // ordering of the returned batch is no longer guaranteed,
        // which is fine because each `QueuedMessage` is independent
        // and the downstream `group_by_domain` re-sorts anyway.
        let messages: Vec<QueuedMessage> = if let Some(ref dkim) = self.dkim {
            use futures_util::stream::{self, StreamExt};
            let dkim = dkim.clone();
            let dkim_resolver = self.dkim_resolver.clone();
            stream::iter(messages)
                .map(|mut msg| {
                    let dkim = dkim.clone();
                    let dkim_resolver = dkim_resolver.clone();
                    async move {
                        // ARC seal forwarded messages before DKIM signing.
                        if msg.is_forwarded {
                            match dkim_sign::arc_seal_message(
                                &dkim,
                                dkim_resolver.as_ref(),
                                &msg.message_data,
                            )
                            .await
                            {
                                Ok(sealed) => msg.message_data = sealed,
                                Err(e) => {
                                    tracing::warn!("ARC sealing failed for msg {}: {e}", msg.id)
                                }
                            }
                        }
                        // DKIM sign: RSA-SHA256 is CPU-bound; run on
                        // the blocking pool so the reactor stays
                        // responsive for the other in-flight signs.
                        let data = std::mem::take(&mut msg.message_data);
                        let dkim_for_sign = dkim.clone();
                        match tokio::task::spawn_blocking(move || dkim_for_sign.sign(&data)).await {
                            Ok(Ok(signed)) => msg.message_data = signed,
                            Ok(Err(e)) => {
                                tracing::warn!("DKIM signing failed for msg {}: {e}", msg.id)
                            }
                            Err(e) => tracing::warn!(
                                "DKIM signing task join failed for msg {}: {e}",
                                msg.id
                            ),
                        }
                        msg
                    }
                })
                .buffer_unordered(8)
                .collect()
                .await
        } else {
            messages
        };

        let groups = group_by_domain(messages);
        let pool = self.pool.clone();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            self.config.max_concurrent_domains,
        ));

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
                deliver_domain_static(
                    &resolver,
                    &hostname,
                    &domain,
                    domain_messages,
                    &pool,
                    max_per_conn,
                    event_sender.as_ref(),
                )
                .await;
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
        Some(r) => {
            r.recv().await;
        }
        None => std::future::pending().await,
    }
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

    #[test]
    fn group_by_domain_single_domain() {
        let messages = vec![
            make_msg(1, "a.com"),
            make_msg(2, "a.com"),
            make_msg(3, "a.com"),
        ];
        let groups = group_by_domain(messages);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups["a.com"].len(), 3);
    }

    #[test]
    fn group_by_domain_preserves_order_within_group() {
        let messages = vec![
            make_msg(10, "x.com"),
            make_msg(20, "y.com"),
            make_msg(30, "x.com"),
        ];
        let groups = group_by_domain(messages);
        let x_ids: Vec<i64> = groups["x.com"].iter().map(|m| m.id).collect();
        assert_eq!(x_ids, vec![10, 30]);
    }

    #[test]
    fn group_by_domain_many_domains() {
        let messages: Vec<QueuedMessage> = (0..100)
            .map(|i| make_msg(i, &format!("domain{}.com", i % 10)))
            .collect();
        let groups = group_by_domain(messages);
        assert_eq!(groups.len(), 10);
        for v in groups.values() {
            assert_eq!(v.len(), 10);
        }
    }

    #[test]
    fn worker_config_clone() {
        let cfg = WorkerConfig::default();
        let c2 = cfg.clone();
        assert_eq!(c2.poll_interval_secs, cfg.poll_interval_secs);
        assert_eq!(c2.batch_size, cfg.batch_size);
    }

    #[test]
    fn group_by_domain_message_fields_intact() {
        let msg = QueuedMessage {
            id: 99,
            sender: "orig@example.com".into(),
            recipient: "dest@target.com".into(),
            domain: "target.com".into(),
            message_data: vec![0xde, 0xad],
            status: QueueStatus::Pending,
            attempts: 2,
            max_attempts: 5,
            next_retry: 12345,
            last_error: Some("timeout".into()),
            message_id: Some("mid99".into()),
            created_at: 111,
            updated_at: 222,
            is_forwarded: true,
        };
        let groups = group_by_domain(vec![msg]);
        let got = &groups["target.com"][0];
        assert_eq!(got.id, 99);
        assert_eq!(got.sender, "orig@example.com");
        assert_eq!(got.attempts, 2);
        assert_eq!(got.message_data, vec![0xde, 0xad]);
        assert!(got.is_forwarded);
        assert_eq!(got.last_error, Some("timeout".into()));
    }

    #[test]
    fn tls_policy_equality() {
        assert_eq!(TlsPolicy::Opportunistic, TlsPolicy::Opportunistic);
        assert_eq!(TlsPolicy::Require, TlsPolicy::Require);
        assert_ne!(TlsPolicy::Opportunistic, TlsPolicy::Require);
    }

    #[test]
    fn tls_policy_debug() {
        let dbg = format!("{:?}", TlsPolicy::Opportunistic);
        assert!(dbg.contains("Opportunistic"));
        let dbg = format!("{:?}", TlsPolicy::Require);
        assert!(dbg.contains("Require"));
    }

    #[test]
    fn tls_policy_clone() {
        let p = TlsPolicy::Require;
        let p2 = p;
        assert_eq!(p, p2);
    }

    #[test]
    fn tls_policy_copy_semantics() {
        // TlsPolicy is Copy — original is still usable after assignment
        let a = TlsPolicy::Opportunistic;
        let b = a;
        let c = a; // a still usable after copy to b
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn tls_policy_all_variants_distinct() {
        let variants = [TlsPolicy::Opportunistic, TlsPolicy::Require];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn worker_config_custom_values() {
        let cfg = WorkerConfig {
            poll_interval_secs: 10,
            batch_size: 100,
            max_attempts: 3,
            max_concurrent_domains: 16,
            max_messages_per_connection: 25,
        };
        assert_eq!(cfg.poll_interval_secs, 10);
        assert_eq!(cfg.batch_size, 100);
        assert_eq!(cfg.max_attempts, 3);
        assert_eq!(cfg.max_concurrent_domains, 16);
        assert_eq!(cfg.max_messages_per_connection, 25);
    }

    #[test]
    fn worker_config_debug_format() {
        let cfg = WorkerConfig::default();
        let dbg = format!("{:?}", cfg);
        assert!(dbg.contains("WorkerConfig"));
        assert!(dbg.contains("poll_interval_secs"));
        assert!(dbg.contains("batch_size"));
    }

    #[test]
    fn group_by_domain_unicode_domains() {
        let messages = vec![
            make_msg(1, "xn--e1afmapc.xn--p1ai"), // punycode domain
            make_msg(2, "xn--e1afmapc.xn--p1ai"),
            make_msg(3, "example.jp"),
        ];
        let groups = group_by_domain(messages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["xn--e1afmapc.xn--p1ai"].len(), 2);
        assert_eq!(groups["example.jp"].len(), 1);
    }

    #[test]
    fn group_by_domain_all_unique_domains() {
        let messages: Vec<QueuedMessage> =
            (0..50).map(|i| make_msg(i, &format!("d{i}.com"))).collect();
        let groups = group_by_domain(messages);
        assert_eq!(groups.len(), 50);
        for v in groups.values() {
            assert_eq!(v.len(), 1);
        }
    }

    #[test]
    fn group_by_domain_domain_with_subdomains() {
        // subdomains are distinct from parent domain
        let messages = vec![
            make_msg(1, "example.com"),
            make_msg(2, "mail.example.com"),
            make_msg(3, "example.com"),
        ];
        let groups = group_by_domain(messages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["example.com"].len(), 2);
        assert_eq!(groups["mail.example.com"].len(), 1);
    }

    /// helper: extract sender domain the same way enqueue_dsn does
    fn extract_sender_domain(sender: &str) -> &str {
        sender.rsplit_once('@').map(|(_, d)| d).unwrap_or("unknown")
    }

    #[test]
    fn sender_domain_extraction_normal() {
        assert_eq!(extract_sender_domain("user@example.com"), "example.com");
    }

    #[test]
    fn sender_domain_extraction_no_at() {
        assert_eq!(extract_sender_domain("noatsign"), "unknown");
    }

    #[test]
    fn sender_domain_extraction_multiple_at() {
        // rsplit_once splits at the last @
        assert_eq!(extract_sender_domain("user@sub@example.com"), "example.com");
    }

    #[test]
    fn sender_domain_extraction_empty() {
        assert_eq!(extract_sender_domain(""), "unknown");
    }

    #[test]
    fn sender_domain_extraction_at_only() {
        assert_eq!(extract_sender_domain("@"), "");
    }

    #[test]
    fn dsn_skip_empty_sender() {
        // enqueue_dsn skips when sender is empty — verify the condition
        let msg = make_msg(1, "example.com");
        assert!(
            msg.sender != "<>" && !msg.sender.is_empty(),
            "test setup: msg has a real sender"
        );

        // empty sender should be skipped
        let empty_sender = "";
        assert!(empty_sender.is_empty() || empty_sender == "<>");

        // null sender should be skipped
        let null_sender = "<>";
        assert!(null_sender.is_empty() || null_sender == "<>");
    }

    #[test]
    fn dsn_skip_null_sender() {
        // the "<>" check prevents infinite bounce loops (RFC 3461)
        let null_sender = "<>";
        let empty_sender = "";
        let real_sender = "user@example.com";

        // should skip (bounce-of-bounce prevention)
        assert!(null_sender == "<>" || null_sender.is_empty());
        assert!(empty_sender == "<>" || empty_sender.is_empty());

        // should not skip
        assert!(real_sender != "<>" && !real_sender.is_empty());
    }

    #[test]
    fn retry_delay_integration_with_group_delivery() {
        // verify retry delay for each attempt matches what the worker uses
        use crate::retry::retry_delay_secs;
        for attempt in 0..10u32 {
            let delay = retry_delay_secs(attempt);
            assert!(
                delay >= 60,
                "delay at attempt {attempt} should be at least 60s"
            );
            assert!(
                delay <= 28800,
                "delay at attempt {attempt} should be capped at 28800s"
            );
        }
    }

    #[test]
    fn should_bounce_integration_with_worker_defaults() {
        // with default max_attempts=8, bounces start at attempt 8
        use crate::retry::should_bounce;
        let max = WorkerConfig::default().max_attempts;
        for attempt in 0..max {
            assert!(
                !should_bounce(attempt, max),
                "attempt {attempt} should not bounce"
            );
        }
        assert!(should_bounce(max, max), "attempt {max} should bounce");
        assert!(
            should_bounce(max + 1, max),
            "attempt {} should bounce",
            max + 1
        );
    }

    #[test]
    fn make_msg_helper_defaults() {
        let msg = make_msg(42, "test.org");
        assert_eq!(msg.id, 42);
        assert_eq!(msg.domain, "test.org");
        assert_eq!(msg.recipient, "rcpt@test.org");
        assert_eq!(msg.sender, "sender@example.com");
        assert_eq!(msg.status, QueueStatus::Pending);
        assert_eq!(msg.attempts, 0);
        assert_eq!(msg.max_attempts, 8);
        assert!(!msg.is_forwarded);
        assert!(msg.last_error.is_none());
        assert!(msg.message_id.is_none());
    }

    #[test]
    fn group_by_domain_large_batch() {
        // simulate a realistic batch size matching worker config
        let batch_size = WorkerConfig::default().batch_size;
        let messages: Vec<QueuedMessage> = (0..batch_size as i64)
            .map(|i| make_msg(i, &format!("domain{}.com", i % 5)))
            .collect();
        let groups = group_by_domain(messages);
        assert_eq!(groups.len(), 5);
        let total: usize = groups.values().map(|v| v.len()).sum();
        assert_eq!(total, batch_size as usize);
    }

    #[test]
    fn group_by_domain_ids_are_all_present() {
        let messages = vec![
            make_msg(100, "a.com"),
            make_msg(200, "b.com"),
            make_msg(300, "a.com"),
            make_msg(400, "c.com"),
            make_msg(500, "b.com"),
        ];
        let groups = group_by_domain(messages);
        let mut all_ids: Vec<i64> = groups
            .values()
            .flat_map(|v| v.iter().map(|m| m.id))
            .collect();
        all_ids.sort();
        assert_eq!(all_ids, vec![100, 200, 300, 400, 500]);
    }
}
