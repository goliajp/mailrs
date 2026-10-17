use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::SinkExt;
use hickory_resolver::TokioResolver;
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::{MAX_MESSAGE_SIZE, MAX_RECIPIENTS, Session, SessionConfig};

/// connection idle timeout: close if no command received within this duration
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(300);

/// timeout waiting for DATA content after 354 response
const DATA_TIMEOUT: Duration = Duration::from_secs(600);

use mailrs_core::event_bus::{EventBus, SmtpEvent, next_connection_id};
use mailrs_smtp_codec::{SmtpCodec, SmuggleProtection};

use crate::AccountStore;
use crate::ConnectionMetrics;
use crate::QuotaStore;
use crate::inbound::auth_guard::AuthGuardStore;
use crate::inbound::rate_limit::RateLimitStore;
use mailrs_core::users::UserStore;
use mailrs_outbound_queue::{Notifier, QueueStore};
use mailrs_tls_reload::TlsState;

pub struct ConnectionContext {
    pub hostname: String,
    pub maildir_root: String,
    pub tls_state: Option<TlsState>,
    pub users: Arc<UserStore>,
    pub event_bus: EventBus,
    /// Connection + inbound-verdict metrics sink. Abstracted as
    /// [`ConnectionMetrics`] so the receiver records metrics without holding
    /// the whole spg/kevy-bound `WebState`.
    pub metrics: Arc<dyn ConnectionMetrics>,
    pub rate_limiter: Arc<dyn RateLimitStore>,
    /// Local domains and the message size limit, read again while running.
    pub runtime: crate::runtime_settings::RuntimeSettings,
    /// What this organisation calls itself; see `config::Config`.
    pub org_names: Vec<String>,
    /// Domains allowed to carry that name.
    pub org_name_allowed_domains: Vec<String>,
    /// Outbound enqueue seam for the receiving path: relay recipients,
    /// sieve redirect/vacation copies, and FBL suppression. Abstracted as
    /// [`QueueStore`] so the receiver enqueues without binding the spg
    /// `BackendPool` — the in-process [`mailrs_outbound_queue::PgQueueStore`]
    /// today, a network store in the receiver-split topology. `None` when
    /// no outbound queue is configured (degraded mode / tests).
    pub outbound_enqueue: Option<Arc<dyn QueueStore>>,
    pub resolver: Option<Arc<TokioResolver>>,
    pub dnsbl_zones: Vec<String>,
    pub dnsbl_enabled: bool,
    /// `true` when the SPF/DKIM/ARC/DMARC + content-scan pipeline
    /// should run on inbound mail. Mirrors `cfg.antispam_enabled`.
    pub antispam_enabled: bool,
    /// Receiver-facing storage-usage lookup for quota enforcement.
    /// Abstracted as [`QuotaStore`] so the receiver doesn't bind the
    /// spg-backed `PgMailboxStore` — the quota *limit* comes from
    /// [`AccountStore::quota`], this supplies the *usage*.
    pub quota_store: Option<Arc<dyn QuotaStore>>,
    pub smuggle_protection: SmuggleProtection,
    pub auth_guard: Arc<dyn AuthGuardStore>,
    /// Receiver-facing account / recipient lookups: resolution, submission
    /// auth, sieve / vacation / quota. Abstracted as [`AccountStore`] so the
    /// receiver doesn't bind the spg-backed `DomainStore` — in-process today,
    /// network-backed in the receiver-split topology.
    pub account_store: Option<Arc<dyn AccountStore>>,
    /// Account password hashes held by the core, for submission auth in
    /// the split topology. `None` in the monolith, which has `account_store`.
    pub credential_store: Option<Arc<dyn crate::CredentialStore>>,
    /// Asked at RCPT whether a local recipient exists; `None` accepts all.
    pub recipient_check: Option<crate::RecipientCheck>,
    /// Wakes the outbound delivery worker after the receiving path
    /// enqueues relay / sieve-redirect / vacation mail. Abstracted as a
    /// trait so the receiver doesn't bind the in-process kevy store —
    /// the in-process [`mailrs_outbound_queue::KevyNotifier`] today, a
    /// network notifier in the receiver-split topology.
    pub queue_notifier: Option<Arc<dyn Notifier>>,
    pub srs_secret: Option<String>,
    pub ldap_config: Option<Arc<mailrs_core::ldap_auth::LdapConfig>>,
    pub inbound_pipeline: mailrs_inbound::Pipeline,
    /// Shared kevy sidecar handle for per-recipient whitelist /
    /// blacklist snapshots (v2.4.1 roadmap Phase 3, RFC-B §3.3).
    /// Read via `crate::spam_lists::load_recipient_lists_async`
    /// right before the antispam pipeline runs. `None` in tests or
    /// when a kevy sidecar isn't configured — the pipeline then
    /// sees empty sets and behaves identically to pre-Phase-3.
    pub spam_lists_client: Option<Arc<crate::kevy_net::KevyNetClient>>,
    /// Group-commit delivery executor. Accumulates per-path
    /// Maildir deliveries from concurrent SMTP sessions and flushes
    /// them as a single `deliver_batch` call — see
    /// [`mailrs_delivery_executor`] for tuning + rationale.
    pub delivery_executor: mailrs_delivery_executor::DeliveryExecutor,
    /// Sender to the single post-delivery consumer (S1.4). The DATA handler
    /// try_sends delivered messages here so maildir write stays synchronous
    /// while indexing + the post-delivery pass run off the hot path.
    pub process_tx: delivered::ProcessTx,
    /// P6 receiver/core split: when set (the receiver binary), the DATA handler
    /// writes the accepted message to the spool via this sink and emits a
    /// `SpoolDelivered` notify instead of resolving + delivering inline. `None`
    /// (the monolith / tests) keeps the inline `process_tx` delivery path.
    pub spool_sink: Option<Arc<dyn spool_sink::SpoolSink>>,
}

/// The subset of delivery dependencies the recipient-resolution / sieve /
/// relay helpers need — borrowed from either a `ConnectionContext` (monolith)
/// or the core spool consumer (split). Decoupling these helpers from the full
/// `ConnectionContext` lets the core consume a spool file and run the same
/// resolve/sieve/relay path the receiver used to run inline (P6-S7).
pub struct DeliveryDeps<'a> {
    pub local_domains: &'a [String],
    pub account_store: &'a Option<Arc<dyn AccountStore>>,
    pub outbound_enqueue: &'a Option<Arc<dyn QueueStore>>,
    pub queue_notifier: &'a Option<Arc<dyn Notifier>>,
    pub event_bus: &'a EventBus,
    pub hostname: &'a str,
    pub srs_secret: &'a Option<String>,
}

impl ConnectionContext {
    /// Borrow the delivery-relevant fields for the resolve/sieve/relay helpers.
    pub fn delivery_deps<'a>(&'a self, local_domains: &'a [String]) -> DeliveryDeps<'a> {
        DeliveryDeps {
            local_domains,
            account_store: &self.account_store,
            outbound_enqueue: &self.outbound_enqueue,
            queue_notifier: &self.queue_notifier,
            event_bus: &self.event_bus,
            hostname: &self.hostname,
            srs_secret: &self.srs_secret,
        }
    }
}

mod address;
mod admission;
mod auth;
pub(crate) mod credentials;
pub mod delivered;
pub mod delivery;
mod drive;
mod events;
pub mod headers;
pub mod spool_sink;
mod srs;

pub use delivered::{DeliveredMessage, ProcessTx};
pub use spool_sink::SpoolSink;

use drive::drive_session;

/// handle a plain-text SMTP connection (port 25/587), may upgrade via STARTTLS
#[tracing::instrument(name = "smtp.conn", skip(stream, ctx), fields(peer = %addr, tls = false))]
pub async fn handle_plain_connection(
    stream: TcpStream,
    addr: SocketAddr,
    ctx: Arc<ConnectionContext>,
) {
    metrics::counter!("mailrs_smtp_connections_total", "tls" => "plain").increment(1);
    let conn_id = next_connection_id();
    ctx.metrics.on_connect();
    ctx.event_bus.emit(SmtpEvent::ConnectionOpened {
        id: conn_id,
        addr: addr.to_string(),
        tls: false,
    });

    if let Some(refusal) = admission::ConnectChecks::of(&ctx).refusal(addr.ip()).await {
        let mut framed = Framed::new(stream, codec(&ctx, MAX_MESSAGE_SIZE));
        let _ = framed.send(refusal.reply).await;
        ctx.event_bus.emit(SmtpEvent::SpamRejected {
            id: conn_id,
            reason: refusal.reason,
        });
        ctx.metrics.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    let tls_available = ctx.tls_state.is_some();
    let max_size = ctx.runtime.max_message_size.current().await;
    let config = SessionConfig {
        tls_available,
        tls_active: false,
        require_tls_for_auth: tls_available,
        max_size,
        max_recipients: MAX_RECIPIENTS,
    };
    let mut session = Session::new(&ctx.hostname, config);
    let mut framed = Framed::new(stream, codec(&ctx, max_size));

    let greeting = Response::greeting(&ctx.hostname).format_greeting();
    // The banner is the first line of every SMTP conversation. It was
    // written straight to the socket, so the live monitor's transcript
    // began at the client's EHLO with nothing above it.
    ctx.event_bus.emit(SmtpEvent::ResponseSent {
        id: conn_id,
        response: greeting.trim_end().to_string(),
        state_after: format!("{:?}", session.state),
    });
    if framed.send(greeting).await.is_err() {
        ctx.metrics.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    loop {
        let action = drive_session(&mut framed, &mut session, addr, &ctx, conn_id).await;
        match action {
            SessionAction::Continue => continue,
            SessionAction::Close => {
                ctx.metrics.on_disconnect();
                ctx.event_bus
                    .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                return;
            }
            SessionAction::UpgradeTls => {
                let Some(ref tls) = ctx.tls_state else {
                    ctx.metrics.on_disconnect();
                    ctx.event_bus
                        .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                    return;
                };
                let acceptor = tls.acceptor();

                let parts = framed.into_parts();
                if !parts.read_buf.is_empty() {
                    ctx.metrics.on_disconnect();
                    ctx.event_bus
                        .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                    return;
                }
                let tcp_stream = parts.io;

                let tls_stream = match acceptor.accept(tcp_stream).await {
                    Ok(s) => s,
                    Err(_) => {
                        ctx.metrics.on_disconnect();
                        ctx.event_bus
                            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                        return;
                    }
                };

                session.reset_after_tls();
                ctx.event_bus.emit(SmtpEvent::TlsUpgraded { id: conn_id });
                let mut tls_framed = Framed::new(tls_stream, codec(&ctx, max_size));

                loop {
                    let action =
                        drive_session(&mut tls_framed, &mut session, addr, &ctx, conn_id).await;
                    match action {
                        SessionAction::Continue => continue,
                        SessionAction::Close => {
                            ctx.metrics.on_disconnect();
                            ctx.event_bus
                                .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                            return;
                        }
                        SessionAction::UpgradeTls => {
                            ctx.metrics.on_disconnect();
                            ctx.event_bus
                                .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// handle an implicit TLS connection (port 465)
#[tracing::instrument(name = "smtps.conn", skip(stream, ctx), fields(peer = %addr, tls = true))]
pub async fn handle_tls_connection(
    stream: TcpStream,
    addr: SocketAddr,
    ctx: Arc<ConnectionContext>,
) {
    metrics::counter!("mailrs_smtp_connections_total", "tls" => "implicit").increment(1);
    let conn_id = next_connection_id();
    ctx.metrics.on_connect();
    ctx.event_bus.emit(SmtpEvent::ConnectionOpened {
        id: conn_id,
        addr: addr.to_string(),
        tls: true,
    });

    let Some(ref tls) = ctx.tls_state else {
        ctx.metrics.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    };
    let acceptor = tls.acceptor();

    let tls_stream = match acceptor.accept(stream).await {
        Ok(s) => s,
        Err(_) => {
            ctx.metrics.on_disconnect();
            ctx.event_bus
                .emit(SmtpEvent::ConnectionClosed { id: conn_id });
            return;
        }
    };

    let max_size = ctx.runtime.max_message_size.current().await;
    let config = SessionConfig {
        tls_available: true,
        tls_active: true,
        require_tls_for_auth: false,
        max_size,
        max_recipients: MAX_RECIPIENTS,
    };
    let mut session = Session::new(&ctx.hostname, config);
    let mut framed = Framed::new(tls_stream, codec(&ctx, max_size));

    let greeting = Response::greeting(&ctx.hostname).format_greeting();
    // The banner is the first line of every SMTP conversation. It was
    // written straight to the socket, so the live monitor's transcript
    // began at the client's EHLO with nothing above it.
    ctx.event_bus.emit(SmtpEvent::ResponseSent {
        id: conn_id,
        response: greeting.trim_end().to_string(),
        state_after: format!("{:?}", session.state),
    });
    if framed.send(greeting).await.is_err() {
        ctx.metrics.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    loop {
        let action = drive_session(&mut framed, &mut session, addr, &ctx, conn_id).await;
        match action {
            SessionAction::Continue => continue,
            SessionAction::Close | SessionAction::UpgradeTls => {
                ctx.metrics.on_disconnect();
                ctx.event_bus
                    .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                return;
            }
        }
    }
}

fn codec(ctx: &ConnectionContext, max_size: u64) -> SmtpCodec {
    SmtpCodec::new()
        .with_smuggle_protection(ctx.smuggle_protection)
        .with_max_message_size(max_size as usize)
}

enum SessionAction {
    Continue,
    Close,
    UpgradeTls,
}
