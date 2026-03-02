use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use hickory_resolver::TokioResolver;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::{format_ehlo_response, Response};
use mailrs_smtp_proto::session::{AuthStep, Event, Session, SessionConfig, State};
use mailrs_smtp_proto::{parse_command, unstuff_data};
use mailrs_storage_maildir::Maildir;

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
use crate::sieve::{compile_sieve, evaluate_sieve, SieveAction};
use crate::users::UserStore;
use crate::tls::TlsState;
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
        let mut framed = Framed::new(stream, SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection));
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
                let mut framed = Framed::new(stream, SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection));
                let msg = format!("554 5.7.1 Service unavailable; client [{0}] blocked using {zone} ({result:?})", addr.ip());
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
        max_size: 52428800,
    };
    let mut session = Session::new(&ctx.hostname, config);
    let mut framed = Framed::new(stream, SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection));

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
                ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
                return;
            }
            SessionAction::UpgradeTls => {
                let Some(ref tls) = ctx.tls_state else {
                    ctx.web_state.on_disconnect();
                    ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
                    return;
                };
                let acceptor = tls.acceptor();

                let parts = framed.into_parts();
                if !parts.read_buf.is_empty() {
                    ctx.web_state.on_disconnect();
                    ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
                    return;
                }
                let tcp_stream = parts.io;

                let tls_stream = match acceptor.accept(tcp_stream).await {
                    Ok(s) => s,
                    Err(_) => {
                        ctx.web_state.on_disconnect();
                        ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
                        return;
                    }
                };

                session.reset_after_tls();
                ctx.event_bus.emit(SmtpEvent::TlsUpgraded { id: conn_id });
                let mut tls_framed = Framed::new(tls_stream, SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection));

                loop {
                    let action =
                        drive_session(&mut tls_framed, &mut session, addr, &ctx, conn_id).await;
                    match action {
                        SessionAction::Continue => continue,
                        SessionAction::Close => {
                            ctx.web_state.on_disconnect();
                            ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
                            return;
                        }
                        SessionAction::UpgradeTls => {
                            ctx.web_state.on_disconnect();
                            ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
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
        ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    };
    let acceptor = tls.acceptor();

    let tls_stream = match acceptor.accept(stream).await {
        Ok(s) => s,
        Err(_) => {
            ctx.web_state.on_disconnect();
            ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
            return;
        }
    };

    let config = SessionConfig {
        tls_available: true,
        tls_active: true,
        require_tls_for_auth: false,
        max_size: 52428800,
    };
    let mut session = Session::new(&ctx.hostname, config);
    let mut framed = Framed::new(tls_stream, SmtpCodec::new().with_smuggle_protection(ctx.smuggle_protection));

    let greeting = Response::greeting(&ctx.hostname).format_greeting();
    if framed.send(greeting).await.is_err() {
        ctx.web_state.on_disconnect();
        ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
        return;
    }

    loop {
        let action = drive_session(&mut framed, &mut session, addr, &ctx, conn_id).await;
        match action {
            SessionAction::Continue => continue,
            SessionAction::Close | SessionAction::UpgradeTls => {
                ctx.web_state.on_disconnect();
                ctx.event_bus.emit(SmtpEvent::ConnectionClosed { id: conn_id });
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
    let Some(result) = framed.next().await else {
        return SessionAction::Close;
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

            match framed.next().await {
                Some(Ok(SmtpInput::Data(raw))) => {
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
                            let first_rcpt = forward_paths.first().map(|s| s.as_str()).unwrap_or("");
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
                                DeliveryDecision::Junk { auth_header, reason } => {
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
                        if rcpt.split_once('@')
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
                                        let actions = evaluate_sieve(&compiled, &full_message);
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
                                                            ).await;
                                                        if let Some(ref vk) = ctx.valkey {
                                                            mailrs_outbound_queue::queue::notify(&mut vk.clone()).await;
                                                        }
                                                    }
                                                    tracing::info!(
                                                        event = "sieve_redirect",
                                                        user = rcpt,
                                                        target = addr.as_str(),
                                                        "sieve redirected message"
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
                            if let (Some(ref ds), Some(ref mb_store)) = (&ctx.domain_store, &ctx.mailbox_store) {
                                if let Ok(Some(quota)) = ds.get_quota(rcpt).await {
                                    if quota > 0 {
                                        let usage = mb_store.user_storage_usage(rcpt).await;
                                        if usage + msg_size as u64 > quota as u64 {
                                            tracing::warn!(
                                                event = "quota_exceeded",
                                                user = rcpt,
                                                usage = usage,
                                                quota = quota,
                                                "delivery rejected: quota exceeded"
                                            );
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

                                            // resolve thread_id
                                            let thread_id = if !msg_message_id.is_empty() {
                                                // pre-fetch parent thread_id asynchronously
                                                let parent_tid = if !msg_in_reply_to.is_empty() {
                                                    mb_store
                                                        .find_thread_id_by_message_id(&user, &msg_in_reply_to)
                                                        .await
                                                        .ok()
                                                        .flatten()
                                                } else {
                                                    None
                                                };
                                                mailrs_mailbox::threading::resolve_thread_id(
                                                    &msg_message_id,
                                                    &msg_in_reply_to,
                                                    |_| parent_tid.clone(),
                                                )
                                            } else {
                                                String::new()
                                            };

                                            // ensure sieve target folder exists
                                            if rcpt_folder != "INBOX" && rcpt_folder != "Junk" {
                                                let _ = mb_store.create_mailbox(&user, &rcpt_folder).await;
                                            }

                                            let _ = mb_store.index_message(
                                                &user,
                                                &rcpt_folder,
                                                &id.to_string(),
                                                &reverse_path,
                                                rcpt,
                                                &subject,
                                                msg_size as u32,
                                                now,
                                                &msg_message_id,
                                                &msg_in_reply_to,
                                                &thread_id,
                                            ).await;

                                            // emit NewMessage event
                                            let snippet = extract_snippet(&full_message);
                                            ctx.event_bus.emit(SmtpEvent::NewMessage {
                                                user: user.clone(),
                                                thread_id,
                                                sender: reverse_path.clone(),
                                                subject: subject.clone(),
                                                snippet,
                                            });
                                        }
                                    }
                                    Err(_) => ok = false,
                                },
                                Err(_) => ok = false,
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
                                let domain = rcpt
                                    .split_once('@')
                                    .map(|(_, d)| d)
                                    .unwrap_or("unknown");
                                match mailrs_outbound_queue::queue::enqueue_ex(
                                    pool,
                                    &reverse_path,
                                    rcpt,
                                    domain,
                                    &full_message,
                                    None,
                                    now,
                                    *is_fwd,
                                ).await {
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
                            tracing::error!(event = "no_outbound_queue", "outbound queue unavailable, cannot relay");
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
                Some(Ok(SmtpInput::DataRejected)) => {
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
            if let AuthCheck::LockedOut { remaining_secs } = ctx.auth_guard.check(addr.ip(), &username) {
                let resp = Response::new(
                    421,
                    Some(mailrs_smtp_proto::EnhancedCode { class: 4, subject: 7, detail: 0 }),
                    &format!("Too many auth failures, try again in {remaining_secs}s"),
                );
                if framed.send(resp.format()).await.is_err() {
                    return SessionAction::Close;
                }
                return SessionAction::Continue;
            }
            let ok = ctx.users.verify(&username, &password);
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
    let Some(result) = framed.next().await else {
        return SessionAction::Close;
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
                    if let AuthCheck::LockedOut { remaining_secs } = ctx.auth_guard.check(addr.ip(), &username) {
                        let resp = Response::new(
                            421,
                            Some(mailrs_smtp_proto::EnhancedCode { class: 4, subject: 7, detail: 0 }),
                            &format!("Too many auth failures, try again in {remaining_secs}s"),
                        );
                        if framed.send(resp.format()).await.is_err() {
                            return SessionAction::Close;
                        }
                        return SessionAction::Continue;
                    }
                    let ok = ctx.users.verify(&username, &password);
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
                Event::AuthChallenge { response, step: next_step } => {
                    if framed.send(response.format()).await.is_err() {
                        return SessionAction::Close;
                    }
                    Box::pin(handle_auth_continuation(framed, session, next_step, addr, ctx, conn_id)).await
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
        if line.len() > prefix.len()
            && line[..prefix.len()].eq_ignore_ascii_case(&prefix)
        {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
