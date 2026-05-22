use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_maildir::Maildir;
use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::{Session, State};
use mailrs_smtp_proto::unstuff_data;

use crate::codec::{SmtpCodec, SmtpInput};
use crate::domain_store::ResolvedRecipient;
use crate::event_bus::SmtpEvent;
use crate::inbound::pipeline::DeliveryDecision;
use crate::sieve::{compile_sieve, evaluate_sieve_with_envelope, SieveAction};

use super::super::address::is_local_domain;
use super::super::headers::{extract_snippet, format_received_header};
use super::super::post_delivery::post_delivery_process;
use super::super::srs::srs_rewrite;
use super::super::{ConnectionContext, SessionAction, DATA_TIMEOUT};

pub(super) async fn handle_need_data<S>(
    framed: &mut Framed<S, SmtpCodec>,
    session: &mut Session,
    reverse_path: String,
    forward_paths: Vec<String>,
    addr: SocketAddr,
    ctx: &ConnectionContext,
    conn_id: u64,
) -> SessionAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
            if !is_authenticated && ctx.mail_authenticator.is_some() {
                    let ehlo_domain = match &session.state {
                        State::Greeted { domain } => domain.as_str(),
                        State::Authenticated { domain, .. } => domain.as_str(),
                        _ => "unknown",
                    };
                    let first_rcpt =
                        forward_paths.first().map(|s| s.as_str()).unwrap_or("");
                    let mut receive_ctx = mailrs_inbound::ReceiveContext::new(
                        addr.ip(),
                        ehlo_domain,
                        &reverse_path,
                        first_rcpt,
                        full_message.clone(),
                        &ctx.hostname,
                    );
                    let decision = ctx.inbound_pipeline.run(&mut receive_ctx).await;

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

            let msg_size = full_message.len();

            // split recipients into local and remote (pre-size by total
            // recipients to avoid Vec growth allocations during the loop —
            // typical mail has 1-3 recipients but bulk can spike to 100+)
            // remote_rcpts: (address, is_forwarded)
            let mut initial_local: Vec<String> = Vec::with_capacity(forward_paths.len());
            let mut remote_rcpts: Vec<(String, bool)> = Vec::with_capacity(forward_paths.len());
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

            // resolve aliases for local recipients (pre-sized: typically
            // 1:1 with initial_local; alias expansion may add more but
            // the initial size is a good lower bound)
            let mut local_rcpts: Vec<String> = Vec::with_capacity(initial_local.len());
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

                if let Some(ref ds) = ctx.domain_store
                    && let Ok(Some(script)) = ds.get_sieve_script(rcpt).await {
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

                if skip_delivery {
                    continue;
                }

                if let Some((local, domain)) = rcpt.split_once('@') {
                    // check quota before delivery
                    if let (Some(ds), Some(mb_store)) =
                        (&ctx.domain_store, &ctx.mailbox_store)
                        && let Ok(Some(quota)) = ds.get_quota(rcpt).await
                            && quota > 0 {
                                let usage = mb_store.user_storage_usage(rcpt).await;
                                if usage + msg_size as u64 > quota as u64 {
                                    eprintln!("smtp: quota exceeded for user={rcpt} (usage={usage} bytes, quota={quota} bytes)");
                                    ok = false;
                                    continue;
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
                                    // single-pass extract: mail-parser builds a
                                    // full Message tree per call; calling
                                    // extract_header twice parses twice. The
                                    // _and_from helper hands back both fields
                                    // for one parse.
                                    let (subject, from_header) =
                                        super::super::headers::extract_subject_and_from(
                                            &full_message,
                                        );
                                    // use From header for sender (includes
                                    // display name); fall back to SMTP envelope
                                    // reverse_path
                                    let sender = if from_header.is_empty() {
                                        reverse_path.clone()
                                    } else {
                                        from_header
                                    };

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

                                    let indexed_uid = mb_store
                                        .index_message(
                                            &user,
                                            &rcpt_folder,
                                            &id.to_string(),
                                            &sender,
                                            rcpt,
                                            &subject,
                                            msg_size as u32,
                                            now,
                                            &effective_message_id,
                                            &msg_in_reply_to,
                                            &thread_id,
                                        )
                                        .await
                                        .ok();

                                    // emit NewMessage event
                                    let snippet = extract_snippet(&full_message);
                                    ctx.event_bus.emit(SmtpEvent::NewMessage {
                                        user: user.clone(),
                                        thread_id: thread_id.clone(),
                                        sender: sender.clone(),
                                        subject: subject.clone(),
                                        snippet,
                                    });

                                    // MRS-4: detect iTIP / iMIP invite parts
                                    // and project the parsed payload onto
                                    // messages.invite_payload so the web
                                    // client / macapp can render an
                                    // invite card without re-parsing.
                                    if let Some(uid) = indexed_uid
                                        && let Some(extracted) =
                                            crate::calendar::invite_extract::extract_invite_part(
                                                &full_message,
                                            )
                                        {
                                            if let Ok(parsed) = mailrs_ical::parse_invite(
                                                &extracted.ics_bytes,
                                            ) {
                                                if let Ok(payload_json) =
                                                    serde_json::to_value(&parsed)
                                                {
                                                    let method_str = format!(
                                                        "{:?}",
                                                        parsed.method
                                                    )
                                                    .to_uppercase();
                                                    match mb_store
                                                        .update_invite_payload(
                                                            &user,
                                                            &rcpt_folder,
                                                            uid,
                                                            &payload_json,
                                                            &method_str,
                                                        )
                                                        .await
                                                    {
                                                        Ok(Some(msg_id)) => {
                                                            ctx.event_bus.emit(
                                                                SmtpEvent::InviteReceived {
                                                                    user: user.clone(),
                                                                    message_id: msg_id,
                                                                    method: method_str.clone(),
                                                                    uid: parsed.uid.clone(),
                                                                },
                                                            );
                                                            // MRS-7: reconcile against
                                                            // the user's own calendar
                                                            // (only touches events the
                                                            // user has already RSVP'd).
                                                            if let Some(ref pool) =
                                                                ctx.outbound_queue
                                                            {
                                                                let raw_ics = std::str::from_utf8(
                                                                    &extracted.ics_bytes,
                                                                )
                                                                .unwrap_or("");
                                                                match crate::calendar::reconcile::reconcile_inbound_invite(
                                                                    pool,
                                                                    &user,
                                                                    &parsed,
                                                                    raw_ics,
                                                                )
                                                                .await
                                                                {
                                                                    Ok(outcome) => {
                                                                        tracing::info!(
                                                                            user = %user,
                                                                            uid = %parsed.uid,
                                                                            method = %method_str,
                                                                            outcome = ?outcome,
                                                                            "reconcile",
                                                                        );
                                                                    }
                                                                    Err(e) => {
                                                                        tracing::warn!(
                                                                            "reconcile failed for {user}/{}: {e}",
                                                                            parsed.uid,
                                                                        );
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        Ok(None) => {
                                                            tracing::debug!(
                                                                "invite detected but message row not found for {user}/{rcpt_folder}/{uid}"
                                                            );
                                                        }
                                                        Err(e) => {
                                                            tracing::warn!(
                                                                "update_invite_payload failed: {e}"
                                                            );
                                                        }
                                                    }
                                                }
                                            } else {
                                                tracing::debug!(
                                                    "text/calendar part found but ical parse failed for {user}/{rcpt_folder}/{uid}"
                                                );
                                            }
                                        }

                                    // async post-delivery: contact upsert + content extraction + importance scoring
                                    let mb_store_bg = Arc::clone(mb_store);
                                    let user_bg = user.clone();
                                    let sender_bg = sender.clone();
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

            // FBL: check if this is an ARF complaint report to abuse@
            if local_rcpts.iter().any(|r| r.starts_with("abuse@") || r.starts_with("postmaster@"))
                && let Some((reported_addr, feedback_type)) = crate::fbl::parse_arf_report(&full_message) {
                    eprintln!("FBL: received {feedback_type} complaint for {reported_addr}");
                    if let Some(ref queue_pool) = ctx.outbound_queue {
                        let _ = mailrs_outbound_queue::queue::add_suppression(
                            queue_pool,
                            &reported_addr,
                            &format!("FBL complaint: {feedback_type}"),
                            None,
                        ).await;
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
