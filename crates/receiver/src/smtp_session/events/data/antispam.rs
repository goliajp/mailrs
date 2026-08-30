//! `run_antispam` — invoke the inbound pipeline for non-authenticated
//! connections, fold the `DeliveryDecision` into either a final
//! response (Reject/Greylist) or a (possibly Junk-routed)
//! `Continue`. Metrics + `SmtpEvent::SpamRejected` events emitted
//! inside.

use std::net::SocketAddr;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::State;

use crate::inbound::pipeline::DeliveryDecision;
use mailrs_core::event_bus::SmtpEvent;

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
        /// The four-layer verdict, as JSON, when anything was found.
        /// `None` on the ordinary path — see [`SpoolEnvelope`] for why
        /// an absent verdict is the right answer rather than an empty
        /// one.
        ///
        /// [`SpoolEnvelope`]: mailrs_core::spool::SpoolEnvelope
        fraud_verdict: Option<String>,
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
    // Move `full_message` into ReceiveContext (the pipeline owns it
    // during run()) and reclaim it with `mem::take` after — saves a
    // full-body clone on every inbound message. The pipeline does
    // not retain the body past `run().await` so the take is safe.
    let mut receive_ctx = mailrs_inbound::ReceiveContext::new(
        addr.ip(),
        ehlo_domain,
        reverse_path,
        first_rcpt,
        full_message,
        &ctx.hostname,
    );

    // v2.4.1 Phase 3 (RFC-B §3.3) — populate the per-user
    // whitelist / blacklist snapshot for the primary rcpt. Empty on
    // absent client, kevy error, or missing rcpt — all fail-open
    // and the pipeline falls back to the score-based path
    // (identical to pre-Phase-3 behavior). Also lowercase the
    // envelope MAIL FROM into `from_addr` so the lookup match is
    // case-insensitive.
    receive_ctx.from_addr = reverse_path.to_lowercase();
    if !first_rcpt.is_empty() {
        let (wl, bl) = crate::spam_lists::load_recipient_lists_async(
            ctx.spam_lists_client.clone(),
            first_rcpt,
        )
        .await;
        receive_ctx.recipient_whitelist = wl;
        receive_ctx.recipient_blacklist = bl;
    }
    // A colleague's message is not junk. Mail that really comes from
    // one of our own domains — proven by SPF or DKIM, never by the
    // `From:` header alone — takes the same Accept as a whitelisted
    // sender. Mail our own users *send* never reaches here: this whole
    // function runs only for unauthenticated sessions.
    receive_ctx.local_domains = ctx.local_domains.iter().map(|d| d.to_lowercase()).collect();
    // The half of `mailrs_fraud` that needs to know what this
    // organisation is called. The other half — the mailer fingerprint —
    // is read from the message by `ReceiveContext::new`, because it
    // needs no configuration.
    //
    // Read from the **decoded** `From:`: the names arrive base64'd
    // inside `=?UTF-8?B?…?=` in every sample, and a check on the raw
    // header sees only ASCII.
    // The facts, assembled once, then one scan. This was three
    // separate assignments into three separate fields, each with its
    // own extraction — and the two that read the `From` read it
    // differently for a while. One `Facts`, one `scan`, and a new
    // rule needs neither a field nor a line here.
    let decoded_from = mailrs_inbound::identity::from_header(&receive_ctx.message);
    let sender_host = decoded_from
        .rfind('@')
        .map(|at| {
            decoded_from[at + 1..]
                .trim_end_matches('>')
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    let registrable = mailrs_fraud::brand::registrable(&sender_host);
    let domain_seen =
        crate::spam_lists::domain_seen_async(ctx.spam_lists_client.clone(), &sender_host).await;
    let x_mailer = mailrs_inbound::identity::x_mailer_header(&receive_ctx.message);
    let name_deception = mailrs_inbound::deception_in_display_name(&receive_ctx.message);
    let parsed = mailrs_mime::parse(&receive_ctx.message);
    let attachment_names: Vec<String> = parsed
        .attachments()
        .filter_map(|p| p.attachment_filename())
        .map(|f| f.to_string())
        .collect();
    let subject = mailrs_inbound::subject_header(&receive_ctx.message);
    let facts = mailrs_fraud::Facts {
        from: &decoded_from,
        subject: &subject,
        domain: &sender_host,
        registrable: &registrable,
        domain_seen,
        x_mailer: x_mailer.as_deref(),
        has_zero_width: receive_ctx.deception.unjustified_zero_width,
        has_bidi_override: receive_ctx.deception.bidi_override,
        // Narrower than the reading above, and deliberately: that one
        // folds in the subject, where a zero-width space is
        // occasionally legitimate. In a display name it never was —
        // forty in the corpus, forty phishing.
        has_zero_width_in_name: name_deception.unjustified_zero_width,
        has_executable_attachment: mailrs_fraud::attachment::any_executable(
            attachment_names.iter().map(String::as_str),
        ),
        ..mailrs_fraud::Facts::default()
    };
    let policy = mailrs_fraud::Policy {
        org_names: ctx.org_names.clone(),
        our_domains: ctx.local_domains.iter().map(|d| d.to_lowercase()).collect(),
        allowed_domains: ctx.org_name_allowed_domains.clone(),
    };
    receive_ctx.fraud = mailrs_fraud::scan(&facts, &policy);

    let started = std::time::Instant::now();
    let decision = ctx.inbound_pipeline.run(&mut receive_ctx).await;
    // Only when something was found. The verdict costs four small
    // string builds, and this runs on every message that reaches the
    // server — but a message nobody suspected has no finding worth
    // recording, and a verdict on all of them would bury the ones
    // that matter. `to_pipeline_input` is paid a second time here for
    // the same reason: only on the rare path.
    let fraud_verdict = fraud_verdict_json(&receive_ctx, ctx.inbound_pipeline.spam_threshold());
    let full_message: Vec<u8> = std::mem::take(&mut receive_ctx.message);
    tracing::debug!(
        phase = "inbound_pipeline",
        duration_us = started.elapsed().as_micros() as u64,
        msg_size = full_message.len(),
        "stage complete"
    );

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

/// The four-layer verdict for a message something was found in.
///
/// `None` when every layer came out clean, and `None` again if the
/// verdict will not serialise — a receive path must not fail over
/// bookkeeping. The failure is logged rather than swallowed, because
/// a verdict that silently never arrives is the shape this repository
/// keeps finding: a reader guarded on a field nobody writes.
fn fraud_verdict_json(ctx: &mailrs_inbound::ReceiveContext, spam_threshold: f64) -> Option<String> {
    if !ctx.fraud.any() && !ctx.deception.unjustified_zero_width {
        return None;
    }
    let verdict = mailrs_inbound::assess(&ctx.to_pipeline_input(spam_threshold));
    match serde_json::to_string(&verdict) {
        Ok(json) => Some(json),
        Err(e) => {
            tracing::warn!(error = %e, "fraud verdict did not serialise; none recorded");
            None
        }
    }
}
