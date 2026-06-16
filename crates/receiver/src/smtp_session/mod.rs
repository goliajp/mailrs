use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use hickory_resolver::TokioResolver;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use mailrs_smtp_proto::parse_command;
use mailrs_smtp_proto::response::{Response, format_ehlo_response};
use mailrs_smtp_proto::session::{Event, MAX_MESSAGE_SIZE, MAX_RECIPIENTS, Session, SessionConfig};

/// connection idle timeout: close if no command received within this duration
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(300);

/// timeout waiting for DATA content after 354 response
const DATA_TIMEOUT: Duration = Duration::from_secs(600);

use mailrs_core::event_bus::{EventBus, SmtpEvent, next_connection_id};
use mailrs_smtp_codec::{SmtpCodec, SmtpInput, SmuggleProtection};

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
    pub local_domains: Vec<String>,
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
    /// Wakes the outbound delivery worker after the receiving path
    /// enqueues relay / sieve-redirect / vacation mail. Abstracted as a
    /// trait so the receiver doesn't bind the in-process kevy store —
    /// the in-process [`mailrs_outbound_queue::KevyNotifier`] today, a
    /// network notifier in the receiver-split topology.
    pub queue_notifier: Option<Arc<dyn Notifier>>,
    pub srs_secret: Option<String>,
    pub ldap_config: Option<Arc<mailrs_core::ldap_auth::LdapConfig>>,
    pub inbound_pipeline: mailrs_inbound::Pipeline,
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

mod address;
mod auth;
mod credentials;
pub mod delivered;
mod events;
pub mod headers;
pub mod spool_sink;
mod srs;

pub use delivered::{DeliveredMessage, ProcessTx};
pub use spool_sink::SpoolSink;

use events::handle_event;

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

    // rate limit check
    if !ctx.rate_limiter.check(&addr.ip().to_string()).await {
        let mut framed = Framed::new(
            stream,
            SmtpCodec::new()
                .with_smuggle_protection(ctx.smuggle_protection)
                .with_max_message_size(mailrs_smtp_proto::MAX_MESSAGE_SIZE as usize),
        );
        let _ = framed.send(Response::rate_limited().format()).await;
        ctx.event_bus.emit(SmtpEvent::SpamRejected {
            id: conn_id,
            reason: "rate limited".into(),
        });
        ctx.metrics.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    // DNSBL check
    if ctx.dnsbl_enabled
        && !ctx.dnsbl_zones.is_empty()
        && let Some(ref resolver) = ctx.resolver
        && let Some((zone, result)) =
            mailrs_shield::dnsbl::check_dnsbl(resolver, addr.ip(), &ctx.dnsbl_zones).await
    {
        let mut framed = Framed::new(
            stream,
            SmtpCodec::new()
                .with_smuggle_protection(ctx.smuggle_protection)
                .with_max_message_size(mailrs_smtp_proto::MAX_MESSAGE_SIZE as usize),
        );
        let msg = format!(
            "554 5.7.1 Service unavailable; client [{0}] blocked using {zone} ({result:?})",
            addr.ip()
        );
        let _ = framed.send(msg).await;
        ctx.event_bus.emit(SmtpEvent::SpamRejected {
            id: conn_id,
            reason: format!("DNSBL: {zone} ({result:?})"),
        });
        ctx.metrics.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    let tls_available = ctx.tls_state.is_some();
    let config = SessionConfig {
        tls_available,
        tls_active: false,
        require_tls_for_auth: tls_available,
        max_size: MAX_MESSAGE_SIZE,
        max_recipients: MAX_RECIPIENTS,
    };
    let mut session = Session::new(&ctx.hostname, config);
    let mut framed = Framed::new(
        stream,
        SmtpCodec::new()
            .with_smuggle_protection(ctx.smuggle_protection)
            .with_max_message_size(mailrs_smtp_proto::MAX_MESSAGE_SIZE as usize),
    );

    let greeting = Response::greeting(&ctx.hostname).format_greeting();
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
                let mut tls_framed = Framed::new(
                    tls_stream,
                    SmtpCodec::new()
                        .with_smuggle_protection(ctx.smuggle_protection)
                        .with_max_message_size(mailrs_smtp_proto::MAX_MESSAGE_SIZE as usize),
                );

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

    let config = SessionConfig {
        tls_available: true,
        tls_active: true,
        require_tls_for_auth: false,
        max_size: MAX_MESSAGE_SIZE,
        max_recipients: MAX_RECIPIENTS,
    };
    let mut session = Session::new(&ctx.hostname, config);
    let mut framed = Framed::new(
        tls_stream,
        SmtpCodec::new()
            .with_smuggle_protection(ctx.smuggle_protection)
            .with_max_message_size(mailrs_smtp_proto::MAX_MESSAGE_SIZE as usize),
    );

    let greeting = Response::greeting(&ctx.hostname).format_greeting();
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

enum SessionAction {
    Continue,
    Close,
    UpgradeTls,
}

/// process one command from the stream, return what to do next
async fn drive_session<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let result = match tokio::time::timeout(CONNECTION_TIMEOUT, framed.next()).await {
        Ok(Some(result)) => result,
        Ok(None) => return SessionAction::Close,
        Err(_) => {
            let _ = framed
                .send(Response::new(421, None, "Idle timeout, closing connection").format())
                .await;
            return SessionAction::Close;
        }
    };
    let input = match result {
        Ok(input) => input,
        Err(_) => return SessionAction::Close,
    };

    match input {
        SmtpInput::Command(line) => {
            ctx.event_bus.emit(SmtpEvent::CommandReceived {
                id: conn_id,
                command: line.clone(),
                state_before: format!("{:?}", session.state),
            });

            match parse_command(&line) {
                Ok(cmd) => {
                    let is_ehlo = matches!(
                        cmd,
                        mailrs_smtp_proto::Command::Ehlo(_) | mailrs_smtp_proto::Command::Helo(_)
                    );
                    let event = session.handle_command(&cmd);

                    // EHLO/HELO with 250 → multiline response
                    if is_ehlo && matches!(event, Event::Reply(ref r) if r.code == 250) {
                        let caps = session.capabilities();
                        let resp = format_ehlo_response(&session.hostname, &caps);
                        ctx.event_bus.emit(SmtpEvent::ResponseSent {
                            id: conn_id,
                            response: resp.clone(),
                            state_after: format!("{:?}", session.state),
                        });
                        if framed.send(resp).await.is_err() {
                            return SessionAction::Close;
                        }
                        return SessionAction::Continue;
                    }

                    handle_event(framed, session, event, addr, ctx, conn_id).await
                }
                Err(_) => {
                    let resp = Response::syntax_error();
                    if framed.send(resp.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    SessionAction::Continue
                }
            }
        }
        SmtpInput::Data(_) | SmtpInput::DataRejected => SessionAction::Close,
    }
}
