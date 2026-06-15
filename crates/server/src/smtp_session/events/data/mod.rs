use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::{Session, State};
use mailrs_smtp_proto::unstuff_data;

use crate::event_bus::SmtpEvent;
use mailrs_smtp_codec::{SmtpCodec, SmtpInput};

use super::super::headers::format_received_header;
use super::super::process_delivered::DeliveredMessage;
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
                    if let (Some(ds), Some(qs)) = (&ctx.account_store, &ctx.quota_store)
                        && let Ok(Some(quota)) = ds.quota(rcpt).await
                        && quota > 0
                    {
                        let usage = qs.user_storage_usage(rcpt).await;
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
                            // S1.4 + P5: hand the delivered message to the
                            // core-side post-delivery consumer so maildir
                            // write stays on the hot path. Only a plain
                            // `DeliveredMessage` crosses — the consumer owns
                            // the spg/kevy-bound deps. On a full channel,
                            // block on `send` (backpressure to the single
                            // consumer's rate — never a dropped message);
                            // if the consumer is gone (shutdown) the message
                            // is on disk and reconcile will index it. The
                            // receiver always hands off — the core consumer
                            // decides whether to process (it owns the deps;
                            // none = degraded mode, drains and drops).
                            let msg = DeliveredMessage {
                                maildir_id: id.to_string(),
                                user: format!("{local}@{domain}"),
                                rcpt: rcpt.clone(),
                                rcpt_folder: rcpt_folder.clone(),
                                reverse_path: reverse_path.clone(),
                                full_message: full_message.clone(),
                                msg_message_id: msg_message_id.clone(),
                                msg_in_reply_to: msg_in_reply_to.clone(),
                                msg_size,
                            };
                            use tokio::sync::mpsc::error::TrySendError;
                            match ctx.process_tx.try_send(msg) {
                                Ok(()) => {}
                                Err(TrySendError::Full(msg)) => {
                                    let _ = ctx.process_tx.send(msg).await;
                                }
                                Err(TrySendError::Closed(_)) => {
                                    tracing::error!(
                                        event = "process_consumer_gone",
                                        rcpt = %rcpt,
                                        "post-delivery consumer closed; message on disk, reconcile will index"
                                    );
                                }
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
                if let Some(ref queue) = ctx.outbound_enqueue {
                    let _ = queue
                        .add_suppression(
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
                ctx.metrics.on_message_delivered();
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
