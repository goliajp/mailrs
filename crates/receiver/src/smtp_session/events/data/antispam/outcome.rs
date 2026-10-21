//! Folding the pipeline's `DeliveryDecision` into the SMTP reply or the
//! folder the message is delivered to, with the metrics and events
//! each decision emits.

use mailrs_core::event_bus::SmtpEvent;
use mailrs_smtp_proto::response::Response;

use crate::inbound::pipeline::DeliveryDecision;

use super::super::super::super::ConnectionContext;
use super::AntiSpamOutcome;

/// The fraud rules could not run: defer, so the sender retries.
pub(super) fn rules_unavailable(
    error: impl std::fmt::Display,
    ctx: &ConnectionContext,
) -> AntiSpamOutcome {
    tracing::error!(%error, "fraud rules unavailable; deferring message");
    ctx.metrics.inbound_defer();
    metrics::counter!("mailrs_inbound_verdict_total", "verdict" => "defer").increment(1);
    AntiSpamOutcome::Reject(Response::new(
        451,
        None,
        "Temporary classification failure, please retry",
    ))
}

pub(super) fn from_decision(
    decision: DeliveryDecision,
    full_message: Vec<u8>,
    fraud_verdict: Option<String>,
    conn_id: u64,
    ctx: &ConnectionContext,
) -> AntiSpamOutcome {
    match decision {
        DeliveryDecision::Reject { code, message } => {
            ctx.metrics.inbound_reject();
            metrics::counter!("mailrs_inbound_verdict_total", "verdict" => "reject").increment(1);
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
            ctx.metrics.inbound_defer();
            metrics::counter!("mailrs_inbound_verdict_total", "verdict" => "defer").increment(1);
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
            ctx.metrics.inbound_junk();
            metrics::counter!("mailrs_inbound_verdict_total", "verdict" => "junk").increment(1);
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
                fraud_verdict,
            }
        }
        DeliveryDecision::Accept { auth_header } => {
            ctx.metrics.inbound_accept();
            metrics::counter!("mailrs_inbound_verdict_total", "verdict" => "accept").increment(1);
            let mut new_msg = auth_header.into_bytes();
            new_msg.extend_from_slice(&full_message);
            AntiSpamOutcome::Continue {
                full_message: new_msg,
                target_folder: "INBOX",
                fraud_verdict,
            }
        }
    }
}
