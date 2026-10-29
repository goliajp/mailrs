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
use crate::inbound::rate_limit::RateLimitStore;
use mailrs_auth_guard::login::LoginGuard;
use mailrs_core::users::UserStore;
use mailrs_tls_reload::TlsState;

pub struct ConnectionContext {
    pub hostname: String,
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
    /// See `config::ReceiverConfig::trusted_relays`.
    pub trusted_relays: Vec<std::net::IpAddr>,
    pub resolver: Option<Arc<TokioResolver>>,
    pub dnsbl_zones: Vec<String>,
    pub dnsbl_enabled: bool,
    /// `true` when the SPF/DKIM/ARC/DMARC + content-scan pipeline
    /// should run on inbound mail. Mirrors `cfg.antispam_enabled`.
    pub antispam_enabled: bool,
    pub smuggle_protection: SmuggleProtection,
    pub submission_from_check: crate::config::SubmissionFromCheck,
    pub auth_guard: Arc<dyn LoginGuard>,
    /// Submission auth against the in-process domain store. Abstracted as
    /// [`AccountStore`] so the receiver doesn't bind the spg-backed
    /// `DomainStore`. `None` in the receiver binary, which has
    /// `credential_store`.
    pub account_store: Option<Arc<dyn AccountStore>>,
    /// Account password hashes held by the core, for submission auth in
    /// the split topology. `None` in the monolith, which has `account_store`.
    pub credential_store: Option<Arc<dyn crate::CredentialStore>>,
    /// Asked at RCPT whether a local recipient exists; `None` accepts all.
    pub recipient_check: Option<crate::RecipientCheck>,
    pub ldap_config: Option<Arc<mailrs_core::ldap_auth::LdapConfig>>,
    pub inbound_pipeline: mailrs_inbound::Pipeline,
    /// Shared kevy sidecar handle for per-recipient whitelist /
    /// blacklist snapshots (v2.4.1 roadmap Phase 3, RFC-B §3.3).
    /// Read via `crate::spam_lists::load_recipient_lists_async`
    /// right before the antispam pipeline runs. `None` in tests or
    /// when a kevy sidecar isn't configured — the pipeline then
    /// sees empty sets and behaves identically to pre-Phase-3.
    pub spam_lists_client: Option<Arc<crate::kevy_net::KevyNetClient>>,
    /// Where DATA writes each accepted message, for a core to deliver; the
    /// receiver resolves and delivers nothing itself.
    pub spool_sink: Arc<dyn spool_sink::SpoolSink>,
}

mod address;
mod admission;
mod auth;
pub(crate) mod credentials;
mod drive;
mod events;
mod headers;
pub mod spool_sink;

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
