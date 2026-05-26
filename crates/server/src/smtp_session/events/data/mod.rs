use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::{Session, State};
use mailrs_smtp_proto::unstuff_data;

use crate::event_bus::SmtpEvent;
use mailrs_smtp_codec::{SmtpCodec, SmtpInput};

use super::super::headers::{extract_snippet, format_received_header};
use super::super::post_delivery::post_delivery_process;
use super::super::{ConnectionContext, DATA_TIMEOUT, SessionAction};

mod antispam;
mod recipients;
mod remote;
mod sieve;

use antispam::{AntiSpamOutcome, run_antispam};
use recipients::classify_recipients;
use remote::{RemoteEnqueueResult, enqueue_remote_rcpts};
use sieve::apply_sieve_actions;

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

            let mut target_folder = "INBOX";
            if !is_authenticated && ctx.antispam_enabled {
                match run_antispam(
                    &session.state,
                    addr,
                    &reverse_path,
                    &forward_paths,
                    full_message,
                    conn_id,
                    ctx,
                )
                .await
                {
                    AntiSpamOutcome::Reject(resp) => {
                        if framed.send(resp.format()).await.is_err() {
                            return SessionAction::Close;
                        }
                        return SessionAction::Continue;
                    }
                    AntiSpamOutcome::Continue {
                        full_message: new_msg,
                        target_folder: tf,
                    } => {
                        full_message = new_msg;
                        target_folder = tf;
                    }
                }
            }

            let msg_size = full_message.len();

            let (local_rcpts, remote_rcpts) = classify_recipients(&forward_paths, ctx).await;

            let mut ok = true;

            // extract threading headers once
            let msg_message_id = mailrs_mailbox::threading::extract_message_id(&full_message);
            let msg_in_reply_to = mailrs_mailbox::threading::extract_in_reply_to(&full_message);

            // Hand ownership of the message body to an `Arc<Vec<u8>>` —
            // every downstream consumer (delivery_executor, AI
            // post-delivery spawn, content_scan helpers) only needs a
            // ref-counted handle. Shadowing rather than building a
            // separate `shared_body` Arc means we don't pay even one
            // Vec clone for the wrap — the body bytes move into the
            // Arc directly. Subsequent `&full_message` calls work
            // through `Arc<Vec<u8>> → Vec<u8> → [u8]` deref coercion.
            let full_message = std::sync::Arc::new(full_message);

            // deliver to local recipients via maildir
            for rcpt in &local_rcpts {
                let (rcpt_folder, skip_delivery) =
                    apply_sieve_actions(rcpt, target_folder, &reverse_path, &full_message, ctx)
                        .await;
                if skip_delivery {
                    continue;
                }

                if let Some((local, domain)) = rcpt.split_once('@') {
                    // check quota before delivery
                    if let (Some(ds), Some(mb_store)) = (&ctx.domain_store, &ctx.mailbox_store)
                        && let Ok(Some(quota)) = ds.get_quota(rcpt).await
                        && quota > 0
                    {
                        let usage = mb_store.user_storage_usage(rcpt).await;
                        if usage + msg_size as u64 > quota as u64 {
                            tracing::warn!(
                                event = "smtp_quota_exceeded",
                                user = %rcpt,
                                usage_bytes = usage,
                                quota_bytes = quota,
                                "delivery rejected: recipient over quota"
                            );
                            ok = false;
                            continue;
                        }
                    }

                    let path = format!("{}/{domain}/{local}", ctx.maildir_root);
                    // Delivery goes through the group-commit
                    // executor: it accumulates per-path deliveries
                    // from concurrent SMTP sessions and flushes
                    // them as a single deliver_batch call to
                    // mailrs-maildir 1.2. At N=64 batch this is
                    // ~15× faster than per-message fsync (measured
                    // microbench, 2026-05-24). Worst-case added
                    // latency for a single in-flight delivery is
                    // the executor's max_wait (default 10ms).
                    match ctx
                        .delivery_executor
                        .deliver(path.clone(), full_message.clone())
                        .await
                    {
                        Ok(id) => {
                            // index in mailbox store if available
                            if let Some(ref mb_store) = ctx.mailbox_store {
                                // Compute `id.to_string()` once per
                                // delivery — used both as `maildir_id`
                                // for `index_message` and again for the
                                // AI post-delivery background task.
                                let maildir_id_str = id.to_string();
                                let user = format!("{local}@{domain}");
                                let _ = mb_store.ensure_default_mailboxes(&user).await;
                                let now = chrono::Utc::now().timestamp();
                                // single-pass extract: mail-parser builds a
                                // full Message tree per call; calling
                                // extract_header twice parses twice. The
                                // _and_from helper hands back both fields
                                // for one parse.
                                let extract_started = std::time::Instant::now();
                                let (subject, from_header) =
                                    super::super::headers::extract_subject_and_from(&full_message);
                                tracing::debug!(
                                    phase = "extract_subject_from",
                                    duration_us = extract_started.elapsed().as_micros() as u64,
                                    "stage complete"
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
                                            .find_thread_id_by_message_id(&user, &msg_in_reply_to)
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
                                    let _ = mb_store.create_mailbox(&user, &rcpt_folder).await;
                                }

                                let index_started = std::time::Instant::now();
                                let indexed_uid = mb_store
                                    .index_message(
                                        &user,
                                        &rcpt_folder,
                                        &maildir_id_str,
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
                                tracing::debug!(
                                    phase = "mailbox_index_message",
                                    duration_us = index_started.elapsed().as_micros() as u64,
                                    "stage complete"
                                );

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
                                    if let Ok(parsed) =
                                        mailrs_ical::parse_invite(&extracted.ics_bytes)
                                    {
                                        if let Ok(payload_json) = serde_json::to_value(&parsed) {
                                            let method_str =
                                                format!("{:?}", parsed.method).to_uppercase();
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
                                                    ctx.event_bus.emit(SmtpEvent::InviteReceived {
                                                        user: user.clone(),
                                                        message_id: msg_id,
                                                        method: method_str.clone(),
                                                        uid: parsed.uid.clone(),
                                                    });
                                                    // MRS-7: reconcile against
                                                    // the user's own calendar
                                                    // (only touches events the
                                                    // user has already RSVP'd).
                                                    if let Some(ref pool) = ctx.outbound_queue {
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
                                let maildir_id_bg = maildir_id_str.clone();
                                let maildir_root_bg = ctx.maildir_root.clone();
                                let raw_headers = String::from_utf8_lossy(
                                    &full_message[..full_message.len().min(4096)],
                                )
                                .to_string();
                                // Arc-clone the function-level body (cheap refcount
                                // bump) instead of copying its bytes.
                                let full_msg_bg = Arc::clone(&full_message);
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
                                    )
                                    .await;
                                });
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                event = "smtp_maildir_deliver_failed",
                                rcpt = %rcpt,
                                path = %path,
                                error = %e,
                                "maildir delivery returned error"
                            );
                            ok = false;
                        }
                    }
                }
            }

            // FBL: check if this is an ARF complaint report to abuse@
            if local_rcpts
                .iter()
                .any(|r| r.starts_with("abuse@") || r.starts_with("postmaster@"))
                && let Some(report) = mailrs_arf::parse(&full_message)
                && let Some(reported_addr) = report.complainant()
            {
                tracing::info!(
                    event = "fbl_complaint_received",
                    feedback_type = %report.feedback_type,
                    rcpt = %reported_addr,
                    "ARF feedback report parsed, adding to suppression list"
                );
                if let Some(ref queue_pool) = ctx.outbound_queue {
                    let _ = mailrs_outbound_queue::queue::add_suppression(
                        queue_pool,
                        reported_addr,
                        &format!("FBL complaint: {}", report.feedback_type),
                        None,
                    )
                    .await;
                }
            }

            if !remote_rcpts.is_empty() {
                match enqueue_remote_rcpts(
                    &remote_rcpts,
                    &reverse_path,
                    &full_message,
                    is_authenticated,
                    conn_id,
                    ctx,
                )
                .await
                {
                    RemoteEnqueueResult::Ok => {}
                    RemoteEnqueueResult::PartialFailure => {
                        ok = false;
                    }
                    RemoteEnqueueResult::RelayDenied => {
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
                .send(Response::new(421, None, "Data timeout, closing connection").format())
                .await;
            SessionAction::Close
        }
        _ => SessionAction::Close,
    }
}
