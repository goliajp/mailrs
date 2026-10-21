//! `run_antispam` — invoke the inbound pipeline for non-authenticated
//! connections, fold the `DeliveryDecision` into either a final
//! response (Reject/Greylist) or a (possibly Junk-routed)
//! `Continue`. Metrics + `SmtpEvent::SpamRejected` events emitted
//! inside.

use std::net::SocketAddr;

use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::State;

use super::super::super::ConnectionContext;

mod evidence;
mod outcome;

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
    let first_rcpt = forward_paths.first().map(|s| s.as_str()).unwrap_or("");
    let mut receive_ctx = evidence::receive_context(
        session_state,
        addr,
        reverse_path,
        first_rcpt,
        full_message,
        ctx,
    )
    .await;
    let evidence = evidence::Evidence::gather(&receive_ctx, ctx).await;
    let facts = evidence.facts(&receive_ctx, addr);
    let policy = evidence::policy(ctx).await;
    let scan = match mailrs_fraud_lua::scan(&facts, &policy) {
        Ok(scan) => scan,
        Err(error) => return outcome::rules_unavailable(error, ctx),
    };
    receive_ctx.fraud = scan.findings;
    // Which bundle answered, for the verdict below. Both passes run
    // the same one, so the second need not report it again.
    let rules_version = scan.version;

    let started = std::time::Instant::now();
    let decision = ctx.inbound_pipeline.run(&mut receive_ctx).await;
    rescan_with_alignment(&mut receive_ctx, facts, &policy);
    // Only when something was found. The verdict costs four small
    // string builds, and this runs on every message that reaches the
    // server — but a message nobody suspected has no finding worth
    // recording, and a verdict on all of them would bury the ones
    // that matter. `to_pipeline_input` is paid a second time here for
    // the same reason: only on the rare path.
    let fraud_verdict = fraud_verdict_json(
        &receive_ctx,
        ctx.inbound_pipeline.spam_threshold(),
        &rules_version,
    );
    let full_message: Vec<u8> = std::mem::take(&mut receive_ctx.message);
    tracing::debug!(
        phase = "inbound_pipeline",
        duration_us = started.elapsed().as_micros() as u64,
        msg_size = full_message.len(),
        "stage complete"
    );
    outcome::from_decision(decision, full_message, fraud_verdict, conn_id, ctx)
}

/// Scan again, now that alignment is known.
///
/// The first scan cannot see SPF, DKIM or DMARC: the stage that
/// checks them is in the pipeline that just ran.  A rule that turns
/// on alignment — `claims-our-domain`, which asks whether a `From`
/// at one of our own domains was authenticated as ours — therefore
/// declines on the first pass and answers on this one.  The stored
/// verdict is what `ingest.rs` holds a conversation by, so this is
/// the pass that decides it; the accept/junk/reject decision is
/// already made and is not revisited, because the two questions
/// are different ones.
fn rescan_with_alignment(
    receive_ctx: &mut mailrs_inbound::ReceiveContext,
    facts: mailrs_fraud::Facts<'_>,
    policy: &mailrs_fraud::Policy,
) {
    let facts = mailrs_fraud::Facts {
        spf: &receive_ctx.auth_results.spf,
        dkim: &receive_ctx.auth_results.dkim,
        dmarc: &receive_ctx.auth_results.dmarc,
        ..facts
    };
    match mailrs_fraud_lua::scan(&facts, policy) {
        Ok(rescan) => receive_ctx.fraud = rescan.findings,
        // The first pass stands, and is already what the context
        // holds: it ran the same rules on everything but alignment,
        // and a rule set that went away for a moment is not a reason
        // to drop what it found.
        Err(error) => {
            tracing::warn!(%error, "fraud rules unavailable for the alignment pass; keeping the first verdict");
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
fn fraud_verdict_json(
    ctx: &mailrs_inbound::ReceiveContext,
    spam_threshold: f64,
    rules_version: &str,
) -> Option<String> {
    if !ctx.fraud.any() && !ctx.deception.unjustified_zero_width {
        return None;
    }
    let mut verdict = mailrs_inbound::assess(&ctx.to_pipeline_input(spam_threshold));
    verdict.rules_version = rules_version.into();
    match serde_json::to_string(&verdict) {
        Ok(json) => Some(json),
        Err(e) => {
            tracing::warn!(error = %e, "fraud verdict did not serialise; none recorded");
            None
        }
    }
}
