//! `run_antispam` — invoke the inbound pipeline for non-authenticated
//! connections, fold the `DeliveryDecision` into either a final
//! response (Reject/Greylist) or a (possibly Junk-routed)
//! `Continue`. Metrics + `SmtpEvent::SpamRejected` events emitted
//! inside.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::State;

use crate::event_bus::SmtpEvent;
use crate::inbound::pipeline::DeliveryDecision;

use super::super::super::ConnectionContext;

pub(super) enum AntiSpamOutcome {
    /// Pipeline produced a final response (Reject / Greylist). Caller
    /// sends + returns `Continue`.
    Reject(Response),
    /// Continue normal processing with the (possibly auth-header-
    /// prefixed) message + folder.
    Continue {
        full_message: Vec<u8>,
        target_folder: &'static str,
    },
}

pub(super) async fn run_antispam(
    session_state: &State,
    addr: SocketAddr,
    reverse_path: &str,
    forward_paths: &[String],
    full_message: Vec<u8>,
    conn_id: u64,
    ctx: &ConnectionContext,
) -> AntiSpamOutcome {
    let ehlo_domain = match session_state {
        State::Greeted { domain } => domain.as_str(),
        State::Authenticated { domain, .. } => domain.as_str(),
        _ => "unknown",
    };
    let first_rcpt = forward_paths.first().map(|s| s.as_str()).unwrap_or("");
    let mut receive_ctx = mailrs_inbound::ReceiveContext::new(
        addr.ip(),
        ehlo_domain,
        reverse_path,
        first_rcpt,
        full_message.clone(),
        &ctx.hostname,
    );
    let started = std::time::Instant::now();
    let decision = ctx.inbound_pipeline.run(&mut receive_ctx).await;
    tracing::debug!(
        phase = "inbound_pipeline",
        duration_us = started.elapsed().as_micros() as u64,
        msg_size = full_message.len(),
        "stage complete"
    );

    match decision {
        DeliveryDecision::Reject { code, message } => {
            ctx.web_state
                .inbound_reject_total
                .fetch_add(1, Ordering::Relaxed);
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
            AntiSpamOutcome::Reject(resp)
        }
        DeliveryDecision::Greylist => {
            ctx.web_state
                .inbound_defer_total
                .fetch_add(1, Ordering::Relaxed);
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
            AntiSpamOutcome::Reject(resp)
        }
        DeliveryDecision::Junk {
            auth_header,
            reason,
        } => {
            ctx.web_state
                .inbound_junk_total
                .fetch_add(1, Ordering::Relaxed);
            tracing::info!(
                event = "junk",
                id = conn_id,
                reason = %reason,
                "delivering to Junk"
            );
            let mut new_msg = auth_header.into_bytes();
            new_msg.extend_from_slice(&full_message);
            AntiSpamOutcome::Continue {
                full_message: new_msg,
                target_folder: "Junk",
            }
        }
        DeliveryDecision::Accept { auth_header } => {
            ctx.web_state
                .inbound_accept_total
                .fetch_add(1, Ordering::Relaxed);
            let mut new_msg = auth_header.into_bytes();
            new_msg.extend_from_slice(&full_message);
            AntiSpamOutcome::Continue {
                full_message: new_msg,
                target_folder: "INBOX",
            }
        }
    }
}
