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

/// Wire the receiving path in one process: a `ConnectionContext` whose DATA
/// writes accepted mail to the spool under `spool_root` and emits
/// `SpoolDelivered`, and this core's spool drain delivering it into
/// `maildir_root` and the store behind `pool`. SMTP → spool → drain →
/// maildir → index, as production runs it. The two roots should be
/// distinct temp dirs.
///
/// Antispam, DNSBL and TLS are disabled so no DNS / certs are needed; with no
/// local domains configured every recipient counts as ours. The drain ticks
/// every second, since no network kevy wakes it here. The rate-limit bucket
/// is sized far above any test's message count so a burst from `127.0.0.1`
/// never trips the per-IP limiter. Returns the bound port and a clone of the
/// event bus so the caller can subscribe before speaking SMTP.
pub async fn spawn_receiving_server(
    pool: BackendPool,
    maildir_root: String,
    spool_root: String,
) -> (u16, EventBus) {
    let event_bus = EventBus::new(1024);
    let web_state = Arc::new(WebState::new(event_bus.clone()));
    let rate_limiter: Arc<dyn RateLimitStore> =
        Arc::new(InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 100_000,
            refill_rate: 100_000.0,
        }));
    let auth_guard: Arc<dyn AuthGuardStore> = Arc::new(AuthGuard::new(AuthGuardConfig::default()));
    let mailbox_store = Arc::new(mailrs_mailbox::PgMailboxStore::new(pool.clone()));
    let domain = Arc::new(crate::domain_store::DomainStore::new(
        Some(pool),
        None,
        crate::health::HealthState::new(),
    ));
    let core = Arc::new(crate::intake::PgCore {
        dir: crate::intake::PgDirectory::new(domain, maildir_root, None),
        mailbox: mailbox_store,
        event_bus: event_bus.clone(),
    });
    crate::intake::spawn_drain(
        core,
        std::path::Path::new(&spool_root),
        std::time::Duration::from_secs(1),
    );

    let ctx = Arc::new(ConnectionContext {
        hostname: "mx.test.local".to_string(),
        tls_state: None,
        users: Arc::new(UserStore::empty()),
        event_bus: event_bus.clone(),
        metrics: web_state as Arc<dyn mailrs_receiver::ConnectionMetrics>,
        rate_limiter,
        runtime: Vec::new().into(),
        org_names: Vec::new(),
        org_name_allowed_domains: Vec::new(),
        resolver: None,
        dnsbl_zones: Vec::new(),
        dnsbl_enabled: false,
        antispam_enabled: false,
        smuggle_protection: SmuggleProtection::Off,
        auth_guard,
        account_store: None,
        credential_store: None,
        recipient_check: None,
        ldap_config: None,
        inbound_pipeline: mailrs_inbound::Pipeline::builder().build(),
        spam_lists_client: None,
        spool_sink: Arc::new(
            mailrs_receiver::smtp_session::spool_sink::MaildirSpoolSink::new(&spool_root),
        ),
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
