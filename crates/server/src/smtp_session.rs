use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use hickory_resolver::TokioResolver;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::{format_ehlo_response, Response};
use mailrs_smtp_proto::session::{AuthStep, Event, Session, SessionConfig, State, MAX_MESSAGE_SIZE, MAX_RECIPIENTS};
use mailrs_smtp_proto::{parse_command, unstuff_data};
use mailrs_storage_maildir::Maildir;

/// connection idle timeout: close if no command received within this duration
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(300);

/// timeout waiting for DATA content after 354 response
const DATA_TIMEOUT: Duration = Duration::from_secs(600);

use crate::codec::{SmtpCodec, SmtpInput};
use crate::config::SmuggleProtection;
use crate::domain_store::{DomainStore, ResolvedRecipient};
use crate::event_bus::{next_connection_id, EventBus, SmtpEvent};
use mail_auth::MessageAuthenticator;

use crate::dmarc_report::DmarcReportStore;
use crate::inbound::auth_guard::{AuthCheck, AuthGuard};
use crate::inbound::greylist_db::GreylistDb;
use crate::inbound::greylisting::GreylistConfig;
use crate::inbound::pipeline::{self, DeliveryDecision};
use crate::inbound::rate_limit::RateLimiter;
use crate::sieve::{compile_sieve, evaluate_sieve_with_envelope, SieveAction};
use crate::tls::TlsState;
use crate::users::UserStore;
use crate::web::WebState;

pub struct ConnectionContext {
    pub hostname: String,
    pub maildir_root: String,
    pub tls_state: Option<TlsState>,
    pub users: Arc<UserStore>,
    pub event_bus: EventBus,
    pub web_state: Arc<WebState>,
    pub rate_limiter: Arc<RateLimiter>,
    pub local_domains: Vec<String>,
    pub outbound_queue: Option<sqlx::PgPool>,
    pub resolver: Option<Arc<TokioResolver>>,
    pub dnsbl_zones: Vec<String>,
    pub dnsbl_enabled: bool,
    pub greylist_db: Option<Arc<GreylistDb>>,
    pub greylist_config: GreylistConfig,
    pub mail_authenticator: Option<Arc<MessageAuthenticator>>,
    pub spam_score_threshold: f64,
    pub mailbox_store: Option<Arc<mailrs_mailbox::MailboxStore>>,
    pub smuggle_protection: SmuggleProtection,
    pub auth_guard: Arc<AuthGuard>,
    pub domain_store: Option<Arc<DomainStore>>,
    pub dmarc_report_store: Option<Arc<DmarcReportStore>>,
    pub clamav_addr: Option<String>,
    pub valkey: Option<redis::aio::ConnectionManager>,
    pub ai_config: Option<crate::ai_spam::AiSpamConfig>,
    pub srs_secret: Option<String>,
    pub ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
}

/// rewrite envelope sender using SRS (Sender Rewriting Scheme)
/// format: SRS0=hash=tt=original_domain=local_part@local_domain
fn srs_rewrite(sender: &str, local_domain: &str, secret: &str) -> String {
    let Some((local_part, original_domain)) = sender.split_once('@') else {
        return sender.to_string();
    };

    // timestamp tag: days since epoch mod 1024 (10-bit, base32-ish)
    let days = (chrono::Utc::now().timestamp() / 86400) as u32 % 1024;
    let tt = format!("{days:03}");

    // HMAC-SHA1 of the rewritten components
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts any key length");
    mac.update(tt.as_bytes());
    mac.update(original_domain.as_bytes());
    mac.update(local_part.as_bytes());
    let hash = hex::encode(&mac.finalize().into_bytes()[..4]);

    format!("SRS0={hash}={tt}={original_domain}={local_part}@{local_domain}")
}

/// verify credentials against users.toml first, then PG accounts table, then LDAP
async fn verify_credentials(ctx: &ConnectionContext, username: &str, password: &str) -> bool {
    if ctx.users.verify(username, password) {
        return true;
    }
    if let Some(ref ds) = ctx.domain_store {
        if let Ok(Some((_account, hash))) = ds.get_account_with_hash(username).await {
            let valid = if hash.is_empty() {
                false
            } else if hash.starts_with("$argon2") {
                UserStore::verify_hash(password, &hash)
            } else {
                hash == password
            };
            if valid {
                return true;
            }
        } else {
            // constant-time: do dummy argon2 work even when account not found
            crate::users::dummy_verify(password);
        }
    }
    // try LDAP as last fallback
    if let Some(ref ldap) = ctx.ldap_config {
        return ldap.authenticate(username, password).await;
    }
    false
}

/// handle a plain-text SMTP connection (port 25/587), may upgrade via STARTTLS
pub async fn handle_plain_connection(
    stream: TcpStream,
    addr: SocketAddr,
    ctx: Arc<ConnectionContext>,
) {
    let conn_id = next_connection_id();
    ctx.web_state.on_connect();
    ctx.event_bus.emit(SmtpEvent::ConnectionOpened {
        id: conn_id,
        addr: addr.to_string(),
        tls: false,
    });

    // rate limit check
    if !ctx.rate_limiter.check(addr.ip()) {
        let mut framed = Framed::new(
            stream,
            SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection),
        );
        let _ = framed.send(Response::rate_limited().format()).await;
        ctx.event_bus.emit(SmtpEvent::SpamRejected {
            id: conn_id,
            reason: "rate limited".into(),
        });
        ctx.web_state.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    // DNSBL check
    if ctx.dnsbl_enabled && !ctx.dnsbl_zones.is_empty() {
        if let Some(ref resolver) = ctx.resolver {
            if let Some((zone, result)) =
                crate::inbound::dnsbl::check_dnsbl(resolver, addr.ip(), &ctx.dnsbl_zones).await
            {
                let mut framed = Framed::new(
                    stream,
                    SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection),
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
                ctx.web_state.on_disconnect();
                ctx.event_bus
                    .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                return;
            }
        }
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
        SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection),
    );

    let greeting = Response::greeting(&ctx.hostname).format_greeting();
    if framed.send(greeting).await.is_err() {
        ctx.web_state.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    loop {
        let action = drive_session(&mut framed, &mut session, addr, &ctx, conn_id).await;
        match action {
            SessionAction::Continue => continue,
            SessionAction::Close => {
                ctx.web_state.on_disconnect();
                ctx.event_bus
                    .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                return;
            }
            SessionAction::UpgradeTls => {
                let Some(ref tls) = ctx.tls_state else {
                    ctx.web_state.on_disconnect();
                    ctx.event_bus
                        .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                    return;
                };
                let acceptor = tls.acceptor();

                let parts = framed.into_parts();
                if !parts.read_buf.is_empty() {
                    ctx.web_state.on_disconnect();
                    ctx.event_bus
                        .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                    return;
                }
                let tcp_stream = parts.io;

                let tls_stream = match acceptor.accept(tcp_stream).await {
                    Ok(s) => s,
                    Err(_) => {
                        ctx.web_state.on_disconnect();
                        ctx.event_bus
                            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                        return;
                    }
                };

                session.reset_after_tls();
                ctx.event_bus.emit(SmtpEvent::TlsUpgraded { id: conn_id });
                let mut tls_framed = Framed::new(
                    tls_stream,
                    SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection),
                );

                loop {
                    let action =
                        drive_session(&mut tls_framed, &mut session, addr, &ctx, conn_id).await;
                    match action {
                        SessionAction::Continue => continue,
                        SessionAction::Close => {
                            ctx.web_state.on_disconnect();
                            ctx.event_bus
                                .emit(SmtpEvent::ConnectionClosed { id: conn_id });
                            return;
                        }
                        SessionAction::UpgradeTls => {
                            ctx.web_state.on_disconnect();
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
pub async fn handle_tls_connection(
    stream: TcpStream,
    addr: SocketAddr,
    ctx: Arc<ConnectionContext>,
) {
    let conn_id = next_connection_id();
    ctx.web_state.on_connect();
    ctx.event_bus.emit(SmtpEvent::ConnectionOpened {
        id: conn_id,
        addr: addr.to_string(),
        tls: true,
    });

    let Some(ref tls) = ctx.tls_state else {
        ctx.web_state.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    };
    let acceptor = tls.acceptor();

    let tls_stream = match acceptor.accept(stream).await {
        Ok(s) => s,
        Err(_) => {
            ctx.web_state.on_disconnect();
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
        SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection),
    );

    let greeting = Response::greeting(&ctx.hostname).format_greeting();
    if framed.send(greeting).await.is_err() {
        ctx.web_state.on_disconnect();
        ctx.event_bus
            .emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    loop {
        let action = drive_session(&mut framed, &mut session, addr, &ctx, conn_id).await;
        match action {
            SessionAction::Continue => continue,
            SessionAction::Close | SessionAction::UpgradeTls => {
                ctx.web_state.on_disconnect();
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

async fn handle_event<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    event: Event,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match event {
        Event::Reply(resp) => {
            ctx.event_bus.emit(SmtpEvent::ResponseSent {
                id: conn_id,
                response: resp.format().trim_end().to_string(),
                state_after: format!("{:?}", session.state),
            });
            if framed.send(resp.format()).await.is_err() {
                return SessionAction::Close;
            }
            SessionAction::Continue
        }
        Event::NeedData {
            reverse_path,
            forward_paths,
        } => {
            let resp = Response::data_start();
            if framed.send(resp.format()).await.is_err() {
                return SessionAction::Close;
            }
            framed.codec_mut().enter_data_mode();

            // check if sender is authenticated (needed for outbound)
            let is_authenticated = matches!(
                session.state,
                State::Authenticated { .. }
                    | State::MailFrom {
                        username: Some(_),
                        ..
                    }
                    | State::RcptTo {
                        username: Some(_),
                        ..
                    }
            );

            match tokio::time::timeout(DATA_TIMEOUT, framed.next()).await {
                Ok(Some(Ok(SmtpInput::Data(raw)))) => {
                    let body = unstuff_data(&raw);
                    let received = format_received_header(
                        &session.hostname,
                        &ctx.hostname,
                        forward_paths.first().map(|s| s.as_str()).unwrap_or(""),
                        &addr,
                    );
                    let mut full_message = received.into_bytes();
                    full_message.extend_from_slice(&body);

                    // run anti-spam pipeline for non-authenticated connections
                    let mut target_folder = "INBOX";
                    if !is_authenticated {
                        if let Some(ref authenticator) = ctx.mail_authenticator {
                            let ehlo_domain = match &session.state {
                                State::Greeted { domain } => domain.as_str(),
                                State::Authenticated { domain, .. } => domain.as_str(),
                                _ => "unknown",
                            };
                            let first_rcpt =
                                forward_paths.first().map(|s| s.as_str()).unwrap_or("");
                            let decision = pipeline::run_inbound_pipeline(
                                authenticator,
                                &ctx.hostname,
                                addr.ip(),
                                ehlo_domain,
                                &reverse_path,
                                first_rcpt,
                                &full_message,
                                ctx.greylist_db.as_ref(),
                                &ctx.greylist_config,
                                ctx.spam_score_threshold,
                                ctx.dmarc_report_store.as_ref(),
                                ctx.resolver.as_ref(),
                                ctx.clamav_addr.as_deref(),
                                ctx.ai_config.as_ref(),
                                ctx.valkey.as_ref(),
                            )
                            .await;

                            match decision {
                                DeliveryDecision::Reject { code, message } => {
                                    let class = (code / 100) as u8;
                                    let resp = Response::new(
                                        code,
                                        Some(mailrs_smtp_proto::EnhancedCode {
                                            class,
                                            subject: 7,
                                            detail: 1,
                                        }),
                                        &message,
                                    );
                                    ctx.event_bus.emit(SmtpEvent::SpamRejected {
                                        id: conn_id,
                                        reason: message,
                                    });
                                    if framed.send(resp.format()).await.is_err() {
                                        return SessionAction::Close;
                                    }
                                    return SessionAction::Continue;
                                }
                                DeliveryDecision::Greylist => {
                                    let resp = Response::new(
                                        451,
                                        Some(mailrs_smtp_proto::EnhancedCode {
                                            class: 4,
                                            subject: 7,
                                            detail: 1,
                                        }),
                                        "Greylisting in effect, please retry later",
                                    );
                                    ctx.event_bus.emit(SmtpEvent::SpamRejected {
                                        id: conn_id,
                                        reason: "greylisted".into(),
                                    });
                                    if framed.send(resp.format()).await.is_err() {
                                        return SessionAction::Close;
                                    }
                                    return SessionAction::Continue;
                                }
                                DeliveryDecision::Junk {
                                    auth_header,
                                    reason,
                                } => {
                                    tracing::info!(
                                        event = "junk",
                                        id = conn_id,
                                        reason = %reason,
                                        "delivering to Junk"
                                    );
                                    let mut new_msg = auth_header.into_bytes();
                                    new_msg.extend_from_slice(&full_message);
                                    full_message = new_msg;
                                    target_folder = "Junk";
                                }
                                DeliveryDecision::Accept { auth_header } => {
                                    let mut new_msg = auth_header.into_bytes();
                                    new_msg.extend_from_slice(&full_message);
                                    full_message = new_msg;
                                }
                            }
                        }
                    }

                    let msg_size = full_message.len();

                    // split recipients into local and remote
                    // remote_rcpts: (address, is_forwarded)
                    let mut initial_local: Vec<String> = Vec::new();
                    let mut remote_rcpts: Vec<(String, bool)> = Vec::new();
                    for rcpt in &forward_paths {
                        if rcpt
                            .split_once('@')
                            .map(|(_, domain)| is_local_domain(domain, &ctx.local_domains))
                            .unwrap_or(true)
                        {
                            initial_local.push(rcpt.clone());
                        } else {
                            remote_rcpts.push((rcpt.clone(), false));
                        }
                    }

                    // resolve aliases for local recipients
                    let mut local_rcpts: Vec<String> = Vec::new();
                    for rcpt in &initial_local {
                        if let Some(ref ds) = ctx.domain_store {
                            match ds.resolve_recipient(rcpt).await {
                                ResolvedRecipient::Account(addr) => {
                                    local_rcpts.push(addr);
                                }
                                ResolvedRecipient::Group(members) => {
                                    for m in members {
                                        local_rcpts.push(m);
                                    }
                                }
                                ResolvedRecipient::Forward(addrs) => {
                                    for a in addrs {
                                        if a.split_once('@')
                                            .map(|(_, d)| is_local_domain(d, &ctx.local_domains))
                                            .unwrap_or(true)
                                        {
                                            local_rcpts.push(a);
                                        } else {
                                            remote_rcpts.push((a, true));
                                        }
                                    }
                                }
                                ResolvedRecipient::Reject => {
                                    // no alias/account match — deliver to original address
                                    local_rcpts.push(rcpt.to_string());
                                }
                            }
                        } else {
                            local_rcpts.push(rcpt.to_string());
                        }
                    }

                    // deduplicate local recipients (e.g. user both in a group and directly CC'd)
                    local_rcpts.sort_unstable();
                    local_rcpts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

                    let mut ok = true;

                    // extract threading headers once
                    let msg_message_id =
                        mailrs_mailbox::threading::extract_message_id(&full_message);
                    let msg_in_reply_to =
                        mailrs_mailbox::threading::extract_in_reply_to(&full_message);

                    // deliver to local recipients via maildir
                    for rcpt in &local_rcpts {
                        // apply sieve script if available
                        let mut rcpt_folder = target_folder.to_string();
                        let mut skip_delivery = false;

                        if let Some(ref ds) = ctx.domain_store {
                            if let Ok(Some(script)) = ds.get_sieve_script(rcpt).await {
                                match compile_sieve(&script) {
                                    Ok(compiled) => {
                                        let actions = evaluate_sieve_with_envelope(
                                            &compiled,
                                            &full_message,
                                            Some(&reverse_path),
                                            Some(rcpt),
                                        );
                                        for action in &actions {
                                            match action {
                                                SieveAction::Keep => {}
                                                SieveAction::FileInto(folder) => {
                                                    rcpt_folder = folder.clone();
                                                }
                                                SieveAction::Discard => {
                                                    tracing::info!(
                                                        event = "sieve_discard",
                                                        user = rcpt,
                                                        "sieve discarded message"
                                                    );
                                                    skip_delivery = true;
                                                }
                                                SieveAction::Redirect(addr) => {
                                                    if let Some(ref pool) = ctx.outbound_queue {
                                                        let now = chrono::Utc::now().timestamp();
                                                        let domain = addr
                                                            .split_once('@')
                                                            .map(|(_, d)| d)
                                                            .unwrap_or("unknown");
                                                        let _ =
                                                            mailrs_outbound_queue::queue::enqueue(
                                                                pool,
                                                                &reverse_path,
                                                                addr,
                                                                domain,
                                                                &full_message,
                                                                None,
                                                                now,
                                                            )
                                                            .await;
                                                        if let Some(ref vk) = ctx.valkey {
                                                            mailrs_outbound_queue::queue::notify(
                                                                &mut vk.clone(),
                                                            )
                                                            .await;
                                                        }
                                                    }
                                                    tracing::info!(
                                                        event = "sieve_redirect",
                                                        user = rcpt,
                                                        target = addr.as_str(),
                                                        "sieve redirected message"
                                                    );
                                                }
                                                SieveAction::Vacation(addr, reply_body) => {
                                                    if let Some(ref pool) = ctx.outbound_queue {
                                                        let now = chrono::Utc::now().timestamp();
                                                        let domain = addr
                                                            .split_once('@')
                                                            .map(|(_, d)| d)
                                                            .unwrap_or("unknown");
                                                        let _ =
                                                            mailrs_outbound_queue::queue::enqueue(
                                                                pool,
                                                                rcpt,
                                                                addr,
                                                                domain,
                                                                reply_body,
                                                                None,
                                                                now,
                                                            )
                                                            .await;
                                                        if let Some(ref vk) = ctx.valkey {
                                                            mailrs_outbound_queue::queue::notify(
                                                                &mut vk.clone(),
                                                            )
                                                            .await;
                                                        }
                                                    }
                                                    tracing::info!(
                                                        event = "sieve_vacation",
                                                        user = rcpt,
                                                        target = addr.as_str(),
                                                        "sieve vacation auto-reply sent"
                                                    );
                                                }
                                                SieveAction::Reject(reason) => {
                                                    tracing::info!(
                                                        event = "sieve_reject",
                                                        user = rcpt,
                                                        reason = reason.as_str(),
                                                        "sieve rejected message"
                                                    );
                                                    skip_delivery = true;
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            event = "sieve_compile_error",
                                            user = rcpt,
                                            error = e.as_str(),
                                            "failed to compile sieve script"
                                        );
                                    }
                                }
                            }
                        }

                        if skip_delivery {
                            continue;
                        }

                        if let Some((local, domain)) = rcpt.split_once('@') {
                            // check quota before delivery
                            if let (Some(ref ds), Some(ref mb_store)) =
                                (&ctx.domain_store, &ctx.mailbox_store)
                            {
                                if let Ok(Some(quota)) = ds.get_quota(rcpt).await {
                                    if quota > 0 {
                                        let usage = mb_store.user_storage_usage(rcpt).await;
                                        if usage + msg_size as u64 > quota as u64 {
                                            eprintln!("smtp: quota exceeded for user={rcpt} (usage={usage} bytes, quota={quota} bytes)");
                                            ok = false;
                                            continue;
                                        }
                                    }
                                }
                            }

                            let path = format!("{}/{domain}/{local}", ctx.maildir_root);
                            match Maildir::create(&path) {
                                Ok(md) => match md.deliver(&full_message) {
                                    Ok(id) => {
                                        // index in mailbox store if available
                                        if let Some(ref mb_store) = ctx.mailbox_store {
                                            let user = format!("{local}@{domain}");
                                            let _ = mb_store.ensure_default_mailboxes(&user).await;
                                            let now = chrono::Utc::now().timestamp();
                                            let subject = extract_header(&full_message, "Subject");

                                            // generate a synthetic message-id if missing
                                            let effective_message_id = if msg_message_id.is_empty() {
                                                format!("{}.{}@mailrs.local", now, id)
                                            } else {
                                                msg_message_id.clone()
                                            };

                                            // resolve thread_id
                                            let thread_id = {
                                                let parent_tid = if !msg_in_reply_to.is_empty() {
                                                    mb_store
                                                        .find_thread_id_by_message_id(
                                                            &user,
                                                            &msg_in_reply_to,
                                                        )
                                                        .await
                                                        .ok()
                                                        .flatten()
                                                } else {
                                                    None
                                                };
                                                mailrs_mailbox::threading::resolve_thread_id(
                                                    &effective_message_id,
                                                    &msg_in_reply_to,
                                                    |_| parent_tid.clone(),
                                                )
                                            };

                                            // ensure sieve target folder exists
                                            if rcpt_folder != "INBOX" && rcpt_folder != "Junk" {
                                                let _ = mb_store
                                                    .create_mailbox(&user, &rcpt_folder)
                                                    .await;
                                            }

                                            let _ = mb_store
                                                .index_message(
                                                    &user,
                                                    &rcpt_folder,
                                                    &id.to_string(),
                                                    &reverse_path,
                                                    rcpt,
                                                    &subject,
                                                    msg_size as u32,
                                                    now,
                                                    &effective_message_id,
                                                    &msg_in_reply_to,
                                                    &thread_id,
                                                )
                                                .await;

                                            // emit NewMessage event
                                            let snippet = extract_snippet(&full_message);
                                            ctx.event_bus.emit(SmtpEvent::NewMessage {
                                                user: user.clone(),
                                                thread_id,
                                                sender: reverse_path.clone(),
                                                subject: subject.clone(),
                                                snippet,
                                            });

                                            // async post-delivery: contact upsert + content extraction + importance scoring
                                            let mb_store_bg = Arc::clone(mb_store);
                                            let user_bg = user.clone();
                                            let sender_bg = reverse_path.clone();
                                            let maildir_id_bg = id.to_string();
                                            let maildir_root_bg = ctx.maildir_root.clone();
                                            let raw_headers = String::from_utf8_lossy(
                                                &full_message[..full_message.len().min(4096)]
                                            ).to_string();
                                            let full_msg_bg = full_message.clone();
                                            let resolver_bg = ctx.resolver.clone();
                                            tokio::spawn(async move {
                                                post_delivery_process(
                                                    &mb_store_bg,
                                                    &user_bg,
                                                    &sender_bg,
                                                    &maildir_id_bg,
                                                    &maildir_root_bg,
                                                    &raw_headers,
                                                    &full_msg_bg,
                                                    resolver_bg.as_deref(),
                                                ).await;
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("smtp: maildir deliver failed for rcpt={rcpt} path={path}: {e}");
                                        ok = false;
                                    }
                                },
                                Err(e) => {
                                    eprintln!("smtp: maildir create failed for rcpt={rcpt} path={path}: {e}");
                                    ok = false;
                                }
                            }
                        }
                    }

                    // enqueue remote recipients
                    if !remote_rcpts.is_empty() {
                        // non-forwarded remote requires authentication (relay protection)
                        let has_user_remote = remote_rcpts.iter().any(|(_, fwd)| !fwd);
                        if has_user_remote && !is_authenticated {
                            let resp = Response::new(
                                550,
                                Some(mailrs_smtp_proto::EnhancedCode {
                                    class: 5,
                                    subject: 7,
                                    detail: 1,
                                }),
                                "Relay access denied",
                            );
                            if framed.send(resp.format()).await.is_err() {
                                return SessionAction::Close;
                            }
                            return SessionAction::Continue;
                        }

                        if let Some(ref pool) = ctx.outbound_queue {
                            let now = chrono::Utc::now().timestamp();
                            let mut enqueue_ok = false;
                            for (rcpt, is_fwd) in &remote_rcpts {
                                let domain =
                                    rcpt.split_once('@').map(|(_, d)| d).unwrap_or("unknown");
                                // apply SRS rewriting for forwarded messages
                                let envelope_sender = if *is_fwd && !reverse_path.is_empty() {
                                    if let Some(ref secret) = ctx.srs_secret {
                                        srs_rewrite(&reverse_path, &ctx.hostname, secret)
                                    } else {
                                        reverse_path.clone()
                                    }
                                } else {
                                    reverse_path.clone()
                                };
                                match mailrs_outbound_queue::queue::enqueue_ex(
                                    pool,
                                    &envelope_sender,
                                    rcpt,
                                    domain,
                                    &full_message,
                                    None,
                                    now,
                                    *is_fwd,
                                )
                                .await
                                {
                                    Ok(_) => enqueue_ok = true,
                                    Err(e) => {
                                        tracing::error!(event = "enqueue_failed", rcpt = rcpt, error = %e, "failed to enqueue remote recipient");
                                        ok = false;
                                    }
                                }
                            }
                            if enqueue_ok {
                                if let Some(ref vk) = ctx.valkey {
                                    mailrs_outbound_queue::queue::notify(&mut vk.clone()).await;
                                }
                                ctx.event_bus.emit(SmtpEvent::MessageQueued {
                                    id: conn_id,
                                    from: reverse_path.clone(),
                                    to: remote_rcpts.iter().map(|(a, _)| a.clone()).collect(),
                                });
                            }
                        } else if !remote_rcpts.is_empty() {
                            tracing::error!(
                                event = "no_outbound_queue",
                                "outbound queue unavailable, cannot relay"
                            );
                            ok = false;
                        }
                    }

                    if ok && !local_rcpts.is_empty() {
                        ctx.web_state.on_message_delivered();
                        ctx.event_bus.emit(SmtpEvent::MessageDelivered {
                            id: conn_id,
                            from: reverse_path,
                            to: local_rcpts,
                            size: msg_size,
                        });
                    }

                    let resp = if ok {
                        Response::data_ok()
                    } else {
                        Response::new(
                            451,
                            Some(mailrs_smtp_proto::EnhancedCode {
                                class: 4,
                                subject: 3,
                                detail: 0,
                            }),
                            "Local error in processing",
                        )
                    };
                    if framed.send(resp.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    SessionAction::Continue
                }
                Ok(Some(Ok(SmtpInput::DataRejected))) => {
                    let resp = Response::new(
                        550,
                        Some(mailrs_smtp_proto::EnhancedCode {
                            class: 5,
                            subject: 7,
                            detail: 7,
                        }),
                        "SMTP smuggling detected, message rejected",
                    );
                    tracing::warn!(
                        event = "smtp_smuggling",
                        id = conn_id,
                        from = %reverse_path,
                        "SMTP smuggling attempt detected"
                    );
                    if framed.send(resp.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    SessionAction::Continue
                }
                Err(_) => {
                    // data transfer timeout
                    let _ = framed
                        .send(
                            Response::new(421, None, "Data timeout, closing connection").format(),
                        )
                        .await;
                    SessionAction::Close
                }
                _ => SessionAction::Close,
            }
        }
        Event::Shutdown(resp) => {
            let _ = framed.send(resp.format()).await;
            SessionAction::Close
        }
        Event::StartTls(resp) => {
            if framed.send(resp.format()).await.is_err() {
                return SessionAction::Close;
            }
            SessionAction::UpgradeTls
        }
        Event::NeedAuth { username, password } => {
            if let AuthCheck::LockedOut { remaining_secs } =
                ctx.auth_guard.check(addr.ip(), &username)
            {
                let resp = Response::new(
                    421,
                    Some(mailrs_smtp_proto::EnhancedCode {
                        class: 4,
                        subject: 7,
                        detail: 0,
                    }),
                    format!("Too many auth failures, try again in {remaining_secs}s"),
                );
                if framed.send(resp.format()).await.is_err() {
                    return SessionAction::Close;
                }
                return SessionAction::Continue;
            }
            let ok = verify_credentials(ctx, &username, &password).await;
            let resp = if ok {
                ctx.auth_guard.record_success(addr.ip(), &username);
                session.set_authenticated(username.clone());
                ctx.event_bus.emit(SmtpEvent::Authenticated {
                    id: conn_id,
                    username,
                });
                Response::auth_ok()
            } else {
                ctx.auth_guard.record_failure(addr.ip(), &username);
                Response::auth_failed()
            };
            if framed.send(resp.format()).await.is_err() {
                return SessionAction::Close;
            }
            SessionAction::Continue
        }
        Event::AuthChallenge { response, step } => {
            if framed.send(response.format()).await.is_err() {
                return SessionAction::Close;
            }
            handle_auth_continuation(framed, session, step, addr, ctx, conn_id).await
        }
    }
}

async fn handle_auth_continuation<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    step: AuthStep,
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
            let event = session.handle_auth_response(&line, &step);
            match event {
                Event::NeedAuth { username, password } => {
                    if let AuthCheck::LockedOut { remaining_secs } =
                        ctx.auth_guard.check(addr.ip(), &username)
                    {
                        let resp = Response::new(
                            421,
                            Some(mailrs_smtp_proto::EnhancedCode {
                                class: 4,
                                subject: 7,
                                detail: 0,
                            }),
                            format!("Too many auth failures, try again in {remaining_secs}s"),
                        );
                        if framed.send(resp.format()).await.is_err() {
                            return SessionAction::Close;
                        }
                        return SessionAction::Continue;
                    }
                    let ok = verify_credentials(ctx, &username, &password).await;
                    let resp = if ok {
                        ctx.auth_guard.record_success(addr.ip(), &username);
                        session.set_authenticated(username.clone());
                        ctx.event_bus.emit(SmtpEvent::Authenticated {
                            id: conn_id,
                            username,
                        });
                        Response::auth_ok()
                    } else {
                        ctx.auth_guard.record_failure(addr.ip(), &username);
                        Response::auth_failed()
                    };
                    if framed.send(resp.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    SessionAction::Continue
                }
                Event::AuthChallenge {
                    response,
                    step: next_step,
                } => {
                    if framed.send(response.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    Box::pin(handle_auth_continuation(
                        framed, session, next_step, addr, ctx, conn_id,
                    ))
                    .await
                }
                Event::Reply(resp) => {
                    if framed.send(resp.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    SessionAction::Continue
                }
                _ => SessionAction::Close,
            }
        }
        _ => SessionAction::Close,
    }
}

fn format_received_header(
    client_domain: &str,
    server_hostname: &str,
    recipient: &str,
    addr: &SocketAddr,
) -> String {
    let now = chrono::Utc::now().to_rfc2822();
    format!(
        "Received: from {client_domain} ({addr})\r\n\tby {server_hostname} with ESMTP\r\n\tfor <{recipient}>; {now}\r\n"
    )
}

/// extract a header value from raw message bytes (with RFC 2047 decoding)
fn extract_header(message: &[u8], name: &str) -> String {
    // use mail-parser for proper RFC 2047 encoded-word decoding
    if let Some(msg) = mail_parser::MessageParser::default().parse(message) {
        match name.to_lowercase().as_str() {
            "subject" => {
                if let Some(s) = msg.subject() {
                    return s.to_string();
                }
            }
            "from" => {
                if let Some(addr) = msg.from().and_then(|a| a.first()) {
                    return match addr.name() {
                        Some(name) => format!("{} <{}>", name, addr.address().unwrap_or("")),
                        None => addr.address().unwrap_or("").to_string(),
                    };
                }
            }
            _ => {}
        }
    }
    // fallback: naive line extraction
    let text = String::from_utf8_lossy(message);
    let prefix = format!("{name}:");
    for line in text.lines() {
        if line.len() > prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            return line[prefix.len()..].trim().to_string();
        }
        if line.is_empty() {
            break;
        }
    }
    String::new()
}

/// extract a short snippet from the message body for notifications
fn extract_snippet(message: &[u8]) -> String {
    if let Some(msg) = mail_parser::MessageParser::default().parse(message) {
        if let Some(text) = msg.body_text(0) {
            let s: String = text.chars().take(100).collect();
            return s.lines().next().unwrap_or("").to_string();
        }
    }
    String::new()
}

/// check if a domain is in the local domains list
/// if list is empty, all domains are considered local (backwards compatible)
fn is_local_domain(domain: &str, local_domains: &[String]) -> bool {
    if local_domains.is_empty() {
        return true;
    }
    let domain_lower = domain.to_lowercase();
    local_domains.contains(&domain_lower)
}

/// async post-delivery processing: contact upsert, content extraction, importance scoring, BIMI
#[allow(clippy::too_many_arguments)]
async fn post_delivery_process(
    mb_store: &mailrs_mailbox::MailboxStore,
    user: &str,
    sender: &str,
    maildir_id: &str,
    _maildir_root: &str,
    raw_headers: &str,
    full_message: &[u8],
    resolver: Option<&TokioResolver>,
) {
    use crate::html_clean;
    use crate::importance::{self, ImportanceSignals};

    // 1. contact upsert
    let display_name = extract_display_name(sender);
    let is_bulk = html_clean::detect_bulk_sender(raw_headers);
    let is_auto = html_clean::is_automated_sender(sender);

    if let Err(e) = mb_store.upsert_contact_inbound(user, sender, &display_name, is_bulk, is_auto).await {
        tracing::warn!("contact upsert failed for {sender}: {e}");
    }

    // 2. parse and extract content
    let (text_body, html_body, _attachments) = crate::message_util::parse_message(full_message);

    // 3. deep html cleaning
    let (clean_text, has_tracking, is_template_heavy, link_count) = if let Some(ref html) = html_body {
        let result = html_clean::clean_email_html(html);
        (
            Some(result.clean_text),
            result.has_tracking_pixel,
            result.is_template_heavy,
            result.link_count,
        )
    } else {
        // plain text email: clean_text = text_body
        (text_body.clone(), false, false, 0)
    };

    // 4. split quoted content
    let new_content = clean_text.as_deref().map(|t| {
        let (new, _) = html_clean::split_quoted_content(t);
        new
    });

    // 5. importance scoring
    let contact_info = mb_store.get_contact_for_scoring(user, sender).await.ok().flatten();
    let is_reply = mb_store.has_sent_to(user, sender).await.unwrap_or(false);

    let signals = ImportanceSignals {
        is_mutual_contact: contact_info.as_ref().is_some_and(|c| c.is_mutual),
        is_direct_recipient: true, // inbound to this user = direct
        is_reply_to_my_email: is_reply,
        has_action_items: false, // will be updated by AI analysis later
        is_vip_sender: contact_info.as_ref().is_some_and(|c| c.is_vip),
        is_bulk_sender: is_bulk,
        is_mailing_list: contact_info.as_ref().map_or(is_bulk, |c| c.is_mailing_list),
        is_automated: is_auto,
        has_tracking_pixel: has_tracking,
        is_template_heavy,
        text_to_html_ratio: 1.0,
        link_count,
        contact_importance_bias: contact_info.as_ref().map_or(0.0, |c| c.importance_bias),
    };

    let (level, score) = importance::calculate_importance(&signals);

    // 6. persist to database
    if let Ok(Some(msg_id)) = mb_store.get_message_id_by_maildir(user, maildir_id).await {
        if let Err(e) = mb_store.update_message_content(
            msg_id,
            text_body.as_deref(),
            html_body.as_deref(),
            clean_text.as_deref(),
            new_content.as_deref(),
            is_bulk,
            has_tracking,
            level.as_str(),
            score,
        ).await {
            tracing::warn!("update_message_content failed for msg {msg_id}: {e}");
        }

        // 7. BIMI logo lookup
        if let Some(resolver) = resolver {
            let sender_domain = sender
                .rsplit_once('@')
                .or_else(|| {
                    // handle "Name <user@domain>" format
                    sender.rsplit_once('<').and_then(|(_, rest)| {
                        rest.trim_end_matches('>').rsplit_once('@')
                    })
                })
                .map(|(_, d)| d.trim_end_matches('>'));
            if let Some(domain) = sender_domain {
                if let Some(logo_url) = crate::domain_check::lookup_bimi_logo(resolver, domain).await {
                    if let Err(e) = mb_store.update_bimi_logo(msg_id, &logo_url).await {
                        tracing::warn!("BIMI update failed for msg {msg_id}: {e}");
                    }
                }
            }
        }
    }
}

/// extract display name from "Display Name <email@domain>" format
fn extract_display_name(sender: &str) -> String {
    if let Some(angle) = sender.find('<') {
        let name = sender[..angle].trim().trim_matches('"');
        if !name.is_empty() {
            return name.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_display_name_with_angle() {
        assert_eq!(extract_display_name("Alice <alice@example.com>"), "Alice");
        assert_eq!(extract_display_name("\"Bob Smith\" <bob@example.com>"), "Bob Smith");
    }

    #[test]
    fn extract_display_name_bare_email() {
        assert_eq!(extract_display_name("alice@example.com"), "");
    }

    #[test]
    fn empty_list_treats_all_as_local() {
        assert!(is_local_domain("anything.com", &[]));
    }

    #[test]
    fn exact_match() {
        let domains = vec!["example.com".into()];
        assert!(is_local_domain("example.com", &domains));
        assert!(!is_local_domain("other.com", &domains));
    }

    #[test]
    fn case_insensitive() {
        let domains = vec!["example.com".into()];
        assert!(is_local_domain("Example.COM", &domains));
        assert!(is_local_domain("EXAMPLE.COM", &domains));
    }

    #[test]
    fn multiple_domains() {
        let domains = vec!["a.com".into(), "b.com".into()];
        assert!(is_local_domain("a.com", &domains));
        assert!(is_local_domain("b.com", &domains));
        assert!(!is_local_domain("c.com", &domains));
    }

    #[test]
    fn subdomain_not_matched() {
        let domains = vec!["example.com".into()];
        assert!(!is_local_domain("sub.example.com", &domains));
    }

    #[test]
    fn srs_rewrite_format() {
        let result = srs_rewrite("user@example.com", "mx.local", "secret123");
        assert!(result.starts_with("SRS0="), "expected SRS0= prefix, got: {result}");
        assert!(result.ends_with("@mx.local"), "expected @mx.local suffix, got: {result}");
        assert!(result.contains("=example.com=user@"), "expected domain and local part, got: {result}");
    }

    #[test]
    fn srs_rewrite_no_at_passthrough() {
        let result = srs_rewrite("postmaster", "mx.local", "secret");
        assert_eq!(result, "postmaster");
    }

    #[test]
    fn srs_rewrite_deterministic_hash() {
        let a = srs_rewrite("test@example.com", "mx.local", "key1");
        let b = srs_rewrite("test@example.com", "mx.local", "key1");
        assert_eq!(a, b, "same inputs should produce same output");
    }

    #[test]
    fn srs_rewrite_different_secrets() {
        let a = srs_rewrite("test@example.com", "mx.local", "key1");
        let b = srs_rewrite("test@example.com", "mx.local", "key2");
        assert_ne!(a, b, "different secrets should produce different hashes");
    }
}
