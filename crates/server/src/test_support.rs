//! Integration-test support for driving the real receiving pipeline.
//!
//! This is **not** part of the server's runtime API (it is `#[doc(hidden)]`).
//! It exists so the `tests/` integration suite can speak SMTP over a real
//! TCP socket and exercise the production `handle_plain_connection` through
//! a genuine [`ConnectionContext`] — without forcing every heavy internal
//! type to become public API. The test only ever sees the bound port plus
//! the [`EventBus`], and queries the database pool it handed in.

use std::sync::Arc;

use crate::config::SmuggleProtection;
use crate::event_bus::EventBus;
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig, AuthGuardStore};
use crate::inbound::rate_limit::{InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig};
use crate::pg::BackendPool;
use crate::smtp_session::{ConnectionContext, handle_plain_connection};
use crate::users::UserStore;
use crate::web::WebState;

/// Wire a real [`ConnectionContext`] for local delivery + indexing, bind a
/// plaintext SMTP listener on an ephemeral loopback port, and spawn an
/// accept loop that drives the production `handle_plain_connection`.
///
/// Antispam, DNSBL and TLS are disabled so no DNS / certs are needed — the
/// recipient classifies as local because `local_domains` is empty, and
/// delivery flows straight to maildir + the mailbox store built from
/// `pool`. Returns the bound port and a clone of the event bus so the
/// caller can subscribe before speaking SMTP.
///
/// The rate-limit bucket is sized far above any test's message count so a
/// burst from `127.0.0.1` never trips the per-IP limiter.
pub async fn spawn_receiving_server(pool: BackendPool, maildir_root: String) -> (u16, EventBus) {
    let event_bus = EventBus::new(1024);
    let web_state = Arc::new(WebState::new(event_bus.clone()));
    let rate_limiter: Arc<dyn RateLimitStore> =
        Arc::new(InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 100_000,
            refill_rate: 100_000.0,
        }));
    let auth_guard: Arc<dyn AuthGuardStore> = Arc::new(AuthGuard::new(AuthGuardConfig::default()));
    let mailbox_store = Arc::new(mailrs_mailbox::PgMailboxStore::new(pool));

    // Core-side post-delivery consumer owns the deps; the receiver hands
    // off only a plain DeliveredMessage (P5: ProcessDeps relocation).
    let process_deps = Some(Arc::new(crate::smtp_session::ProcessDeps {
        mailbox_store: Arc::clone(&mailbox_store),
        event_bus: event_bus.clone(),
        outbound_queue: None,
        resolver: None,
        maildir_root: maildir_root.clone(),
    }));
    let process_tx = crate::smtp_session::spawn_process_consumer(process_deps);

    let ctx = Arc::new(ConnectionContext {
        hostname: "mx.test.local".to_string(),
        maildir_root,
        tls_state: None,
        users: Arc::new(UserStore::empty()),
        event_bus: event_bus.clone(),
        metrics: web_state as Arc<dyn mailrs_receiver::ConnectionMetrics>,
        rate_limiter,
        local_domains: Vec::new(),
        outbound_enqueue: None,
        resolver: None,
        dnsbl_zones: Vec::new(),
        dnsbl_enabled: false,
        antispam_enabled: false,
        quota_store: Some(
            Arc::new(crate::quota_store::MailboxQuotaStore(mailbox_store))
                as Arc<dyn mailrs_receiver::QuotaStore>,
        ),
        smuggle_protection: SmuggleProtection::Off,
        auth_guard,
        account_store: None,
        queue_notifier: None,
        srs_secret: None,
        ldap_config: None,
        inbound_pipeline: mailrs_inbound::Pipeline::builder().build(),
        delivery_executor: mailrs_delivery_executor::DeliveryExecutor::spawn(),
        process_tx,
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral smtp port");
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn(async move {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => break,
            };
            let ctx = ctx.clone();
            tokio::spawn(async move {
                handle_plain_connection(stream, peer, ctx).await;
            });
        }
    });

    (port, event_bus)
}
