//! SPF + DKIM + ARC + DMARC bundled stage.
//!
//! These four checks share intermediate state (DKIM signature outputs feed
//! the DMARC alignment evaluator, SPF result + From-domain together gate
//! DMARC policy) so they run as a single stage rather than four. Writes
//! every field of `ctx.auth_results` and, on DMARC policy=reject,
//! short-circuits with a 550 reject.

use std::sync::Arc;

use async_trait::async_trait;
use mail_auth::dmarc::verify::DmarcParameters;
use mail_auth::spf::verify::SpfParameters;
use mail_auth::{AuthenticatedMessage, MessageAuthenticator};
use mailrs_inbound::{DeliveryDecision, DmarcPolicy, ReceiveContext, Stage, StageOutcome};

use crate::dmarc_report::{DmarcReportStore, DmarcResultRecord, DmarcStore};

/// Stage that performs SPF, DKIM, ARC, and DMARC verification and records
/// the aggregate result in `ctx.auth_results`. On DMARC policy=reject
/// returns `Decide(Reject)`; otherwise returns `Continue`.
///
/// Currently runs `mail-auth` for production decisions; if a
/// `shadow_spf_resolver` is configured it also runs `mailrs-spf` in
/// shadow mode and `tracing::warn!`s on divergence. This validates
/// `mailrs-spf` against real prod traffic before we fully cut over.
pub struct MailAuthStage {
    authenticator: Arc<MessageAuthenticator>,
    dmarc_report_store: Option<Arc<DmarcReportStore>>,
    shadow_spf_resolver: Option<Arc<mailrs_spf::HickoryResolver>>,
    shadow_dkim_resolver: Option<Arc<mailrs_dkim::HickoryDkimResolver>>,
    shadow_arc_resolver: Option<Arc<mailrs_dkim::HickoryDkimResolver>>,
    shadow_dmarc_resolver: Option<Arc<hickory_resolver::TokioResolver>>,
}

impl MailAuthStage {
    /// Construct a `MailAuthStage`. The optional `DmarcReportStore` records
    /// per-message DMARC outcomes for aggregate reporting.
    pub fn new(
        authenticator: Arc<MessageAuthenticator>,
        dmarc_report_store: Option<Arc<DmarcReportStore>>,
    ) -> Self {
        Self {
            authenticator,
            dmarc_report_store,
            shadow_spf_resolver: None,
            shadow_dkim_resolver: None,
            shadow_arc_resolver: None,
            shadow_dmarc_resolver: None,
        }
    }

    /// Enable shadow-mode SPF validation against `mailrs-spf`. The
    /// shadow result does not affect any decision; it's logged via
    /// `tracing::info!` (matches) or `tracing::warn!` (divergences)
    /// so we can validate the new crate against real prod traffic
    /// before cutting over.
    pub fn with_shadow_spf(mut self, resolver: Arc<mailrs_spf::HickoryResolver>) -> Self {
        self.shadow_spf_resolver = Some(resolver);
        self
    }

    /// Enable shadow-mode DKIM validation against `mailrs-dkim`. Same
    /// pattern as [`with_shadow_spf`](Self::with_shadow_spf) — runs in
    /// parallel to mail-auth's verifier and logs match/divergence.
    pub fn with_shadow_dkim(mut self, resolver: Arc<mailrs_dkim::HickoryDkimResolver>) -> Self {
        self.shadow_dkim_resolver = Some(resolver);
        self
    }

    /// Enable shadow-mode ARC validation against `mailrs-arc` 1.1.
    /// Re-uses the DKIM hickory resolver because `ArcResolver = DkimResolver`.
    /// Runs in parallel to mail-auth's `verify_arc` and logs
    /// match/divergence — validates the new crypto path against real
    /// prod traffic before cutting over.
    pub fn with_shadow_arc(mut self, resolver: Arc<mailrs_dkim::HickoryDkimResolver>) -> Self {
        self.shadow_arc_resolver = Some(resolver);
        self
    }

    /// Enable shadow-mode DMARC validation against `mailrs-dmarc`.
    /// Looks up `_dmarc.<from-domain>` via the hickory resolver,
    /// parses the policy, builds a `DmarcInput` from the SPF / DKIM
    /// (via `mailrs_dkim::verify_all`) we have for this message, runs
    /// `mailrs_dmarc::evaluate`, and compares the coarse pass/fail
    /// verdict against mail-auth's. No decision impact — logs only.
    pub fn with_shadow_dmarc(mut self, resolver: Arc<hickory_resolver::TokioResolver>) -> Self {
        self.shadow_dmarc_resolver = Some(resolver);
        self
    }
}

/// Extract the `@<domain>` part from an RFC 5322 `From:` line.
/// Returns `None` if no `@` is present or the input is malformed.
///
/// Handles both `local@domain` and `Name <local@domain>` forms.
/// Tolerates trailing whitespace / CRLF.
fn from_domain(from_header: &str) -> Option<String> {
    let mut s = from_header.trim();
    if let Some(lt) = s.rfind('<')
        && let Some(gt) = s.rfind('>')
        && gt > lt
    {
        s = &s[lt + 1..gt];
    }
    let at = s.rfind('@')?;
    let domain = s[at + 1..].trim_matches(|c: char| c == '>' || c.is_whitespace());
    if domain.is_empty() {
        None
    } else {
        Some(domain.to_ascii_lowercase())
    }
}

#[async_trait]
impl Stage for MailAuthStage {
    fn name(&self) -> &str {
        "mail_auth"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        let spf_output = self.run_spf(ctx).await;
        self.shadow_check_spf(ctx).await;

        // DKIM + ARC + DMARC require a parsed message; bail out
        // (Continue, since SPF already populated) if parse fails.
        let Some(auth_msg) = AuthenticatedMessage::parse(&ctx.message) else {
            return StageOutcome::Continue;
        };

        let dkim_outputs = self.authenticator.verify_dkim(&auth_msg).await;
        let arc_output = self.authenticator.verify_arc(&auth_msg).await;
        ctx.auth_results.arc = arc_result_str(&arc_output, &auth_msg);
        ctx.auth_results.dkim = dkim_result_str(&dkim_outputs);
        self.shadow_check_arc(ctx).await;
        let shadow_dkim_outputs = self.shadow_check_dkim(ctx).await;

        let mail_from_domain = ctx
            .sender
            .rsplit_once('@')
            .map(|(_, d)| d.to_string())
            .unwrap_or_else(|| ctx.ehlo_domain.clone());
        let dmarc_params =
            DmarcParameters::new(&auth_msg, &dkim_outputs, &mail_from_domain, &spf_output);
        let dmarc_output = self.authenticator.verify_dmarc(dmarc_params).await;
        let dmarc_pass = dmarc_output.dkim_result() == &mail_auth::DmarcResult::Pass
            || dmarc_output.spf_result() == &mail_auth::DmarcResult::Pass;

        self.shadow_check_dmarc(ctx, &shadow_dkim_outputs, dmarc_pass, &mail_from_domain)
            .await;

        self.apply_dmarc_policy(ctx, &dmarc_output, dmarc_pass, &mail_from_domain)
            .await
    }
}

// ---------- helpers extracted from `evaluate` for one-fn-one-thing ----------

impl MailAuthStage {
    /// Run mail-auth's SPF verifier against the envelope and
    /// stash the coarse result into `ctx.auth_results.spf`.
    async fn run_spf(&self, ctx: &mut ReceiveContext) -> mail_auth::SpfOutput {
        let spf_params = SpfParameters::verify_mail_from(
            ctx.client_ip,
            &ctx.ehlo_domain,
            &ctx.hostname,
            &ctx.sender,
        );
        let spf_output = self.authenticator.verify_spf(spf_params).await;
        ctx.auth_results.spf = spf_result_str(spf_output.result());
        spf_output
    }

    /// Run mailrs-spf against the same input; log match/divergence.
    /// No decision impact — observability only. Skipped when no
    /// shadow resolver is configured.
    async fn shadow_check_spf(&self, ctx: &ReceiveContext) {
        let Some(ref shadow) = self.shadow_spf_resolver else {
            return;
        };
        let input = mailrs_spf::VerifyInput {
            ip: ctx.client_ip,
            helo: ctx.ehlo_domain.clone(),
            mail_from: ctx.sender.clone(),
        };
        let shadow_str = mailrs_spf::verify(shadow.as_ref(), &input).await.as_str();
        let spf_str = ctx.auth_results.spf.as_str();
        if shadow_str == spf_str {
            tracing::info!(
                event = "spf_shadow_match",
                spf = %spf_str,
                domain = %input.target_domain(),
                "mailrs-spf matches mail-auth"
            );
        } else {
            tracing::warn!(
                event = "spf_shadow_divergence",
                mail_auth = %spf_str,
                mailrs_spf = %shadow_str,
                domain = %input.target_domain(),
                helo = %ctx.ehlo_domain,
                sender = %ctx.sender,
                "SPF result divergence — mailrs-spf says different from mail-auth"
            );
        }
    }

    /// Run mailrs-arc 1.1's structural + crypto verifier against
    /// the raw message; log match/divergence with mail-auth's ARC
    /// result already in `ctx.auth_results.arc`.
    async fn shadow_check_arc(&self, ctx: &ReceiveContext) {
        let Some(ref shadow) = self.shadow_arc_resolver else {
            return;
        };
        let shadow_coarse = match mailrs_arc::ArcChain::extract(&ctx.message) {
            Ok(None) => "none",
            Err(_) => "fail",
            Ok(Some(chain)) => match mailrs_arc::verify_chain_with_crypto(
                &chain,
                shadow.as_ref(),
                &ctx.message,
            )
            .await
            {
                Ok(mailrs_arc::ChainOutcome::Pass) => "pass",
                Ok(_) | Err(_) => "fail",
            },
        };
        let mail_auth_coarse = ctx.auth_results.arc.as_str();
        if shadow_coarse == mail_auth_coarse {
            tracing::info!(
                event = "arc_shadow_match",
                arc = %mail_auth_coarse,
                "mailrs-arc matches mail-auth"
            );
        } else {
            tracing::warn!(
                event = "arc_shadow_divergence",
                mail_auth = %mail_auth_coarse,
                mailrs_arc = %shadow_coarse,
                sender = %ctx.sender,
                "ARC result divergence — mailrs-arc says different from mail-auth"
            );
        }
    }

    /// Run `mailrs_dkim::verify_all` against the raw message; log
    /// match/divergence vs mail-auth's coarse DKIM verdict; return
    /// the per-signature outputs so the DMARC shadow check can
    /// align by `d=` domain.
    async fn shadow_check_dkim(&self, ctx: &ReceiveContext) -> Vec<mailrs_dkim::SignatureOutput> {
        let Some(ref shadow) = self.shadow_dkim_resolver else {
            return Vec::new();
        };
        let outputs = mailrs_dkim::verify_all(shadow.as_ref(), &ctx.message).await;
        let shadow_coarse = if outputs.is_empty() {
            "none"
        } else if outputs.iter().any(|o| o.is_pass()) {
            "pass"
        } else {
            "fail"
        };
        let mail_auth_coarse = ctx.auth_results.dkim.as_str();
        if shadow_coarse == mail_auth_coarse {
            tracing::info!(
                event = "dkim_shadow_match",
                dkim = %mail_auth_coarse,
                sigs = outputs.len(),
                "mailrs-dkim verify_all matches mail-auth"
            );
        } else {
            tracing::warn!(
                event = "dkim_shadow_divergence",
                mail_auth = %mail_auth_coarse,
                mailrs_dkim = %shadow_coarse,
                sigs = outputs.len(),
                sender = %ctx.sender,
                "DKIM result divergence — mailrs-dkim verify_all says different from mail-auth"
            );
        }
        outputs
    }

    /// Resolve `_dmarc.<from-domain>`, parse the policy, build a
    /// `DmarcInput` from the SPF / per-sig DKIM signals already
    /// computed, evaluate via mailrs-dmarc, and log match/divergence
    /// vs mail-auth's coarse DMARC verdict.
    async fn shadow_check_dmarc(
        &self,
        ctx: &ReceiveContext,
        shadow_dkim_outputs: &[mailrs_dkim::SignatureOutput],
        dmarc_pass: bool,
        mail_from_domain: &str,
    ) {
        let Some(ref dmarc_resolver) = self.shadow_dmarc_resolver else {
            return;
        };
        use hickory_resolver::proto::rr::RData;

        let from_dom = mailrs_rfc5322::Message::new(&ctx.message)
            .header_str("From")
            .and_then(from_domain);
        let Some(from_d) = from_dom else { return };

        let q = format!("_dmarc.{from_d}");
        let Ok(lookup) = dmarc_resolver.txt_lookup(q).await else {
            // No _dmarc TXT — same as policy=none. Skip rather
            // than emit a misleading divergence.
            return;
        };
        let txt: String = lookup
            .answers()
            .iter()
            .filter_map(|r| match &r.data {
                RData::TXT(t) => Some(t.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        let Ok(policy) = mailrs_dmarc::DmarcPolicy::parse(&txt) else {
            // Unparseable policy — same skip rule.
            return;
        };

        let spf_pass = ctx.auth_results.spf == "pass";
        let spf_input = spf_pass.then(|| mailrs_dmarc::SpfResult {
            domain: mail_from_domain.to_string(),
            pass: true,
        });
        let dkim_input = shadow_dkim_outputs
            .iter()
            .filter_map(|o| {
                let d = o.domain();
                if d.is_empty() {
                    None
                } else {
                    Some(mailrs_dmarc::DkimSignatureResult {
                        d_domain: d.to_string(),
                        pass: o.is_pass(),
                    })
                }
            })
            .collect::<Vec<_>>();
        let input = mailrs_dmarc::DmarcInput {
            from_domain: from_d.clone(),
            policy_domain: from_d.clone(),
            spf: spf_input,
            dkim: dkim_input,
        };
        let outcome = mailrs_dmarc::evaluate(&policy, &input);
        let shadow_coarse = if outcome.dmarc_pass { "pass" } else { "fail" };
        let mail_auth_coarse = if dmarc_pass { "pass" } else { "fail" };
        if shadow_coarse == mail_auth_coarse {
            tracing::info!(
                event = "dmarc_shadow_match",
                dmarc = %mail_auth_coarse,
                domain = %from_d,
                "mailrs-dmarc matches mail-auth"
            );
        } else {
            tracing::warn!(
                event = "dmarc_shadow_divergence",
                mail_auth = %mail_auth_coarse,
                mailrs_dmarc = %shadow_coarse,
                domain = %from_d,
                sender = %ctx.sender,
                "DMARC result divergence — mailrs-dmarc says different from mail-auth"
            );
        }
    }

    /// Apply the DMARC policy: pass marks `ctx.auth_results.dmarc =
    /// "pass"`; reject short-circuits with a 5.7.1 reject (and
    /// records the result for aggregate reporting); quarantine /
    /// none label the message and let it through. All non-reject
    /// outcomes also record the result for aggregate reports.
    async fn apply_dmarc_policy(
        &self,
        ctx: &mut ReceiveContext,
        dmarc_output: &mail_auth::DmarcOutput,
        dmarc_pass: bool,
        mail_from_domain: &str,
    ) -> StageOutcome {
        let mut dmarc_quarantine = false;

        if dmarc_pass {
            ctx.auth_results.dmarc = "pass".into();
            ctx.auth_results.dmarc_policy = DmarcPolicy::Pass;
        } else {
            match dmarc_output.policy() {
                mail_auth::dmarc::Policy::Reject => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::Reject;

                    self.record_dmarc(ctx, mail_from_domain, "fail", "reject")
                        .await;

                    tracing::info!(
                        event = "dmarc_reject",
                        domain = mail_from_domain,
                        spf = %ctx.auth_results.spf,
                        dkim = %ctx.auth_results.dkim,
                        "DMARC reject"
                    );

                    return StageOutcome::Decide(DeliveryDecision::Reject {
                        code: 550,
                        message: format!(
                            "5.7.1 DMARC policy reject for domain {mail_from_domain}"
                        ),
                    });
                }
                mail_auth::dmarc::Policy::Quarantine => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::Quarantine;
                    dmarc_quarantine = true;
                }
                mail_auth::dmarc::Policy::None => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::None;
                }
                mail_auth::dmarc::Policy::Unspecified => {
                    ctx.auth_results.dmarc = "none".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::None;
                }
            }
        }

        let disposition = if dmarc_quarantine { "quarantine" } else { "none" };
        self.record_dmarc(
            ctx,
            mail_from_domain,
            ctx.auth_results.dmarc.as_str(),
            disposition,
        )
        .await;

        StageOutcome::Continue
    }

    /// Append one DMARC result row for aggregate reporting; no-op
    /// when no `DmarcReportStore` is configured.
    async fn record_dmarc(
        &self,
        ctx: &ReceiveContext,
        mail_from_domain: &str,
        dmarc_result: &str,
        disposition: &str,
    ) {
        let Some(store) = &self.dmarc_report_store else {
            return;
        };
        let _ = store
            .record_result(&DmarcResultRecord {
                source_ip: ctx.client_ip.to_string(),
                from_domain: mail_from_domain.to_string(),
                spf_result: ctx.auth_results.spf.clone(),
                dkim_result: ctx.auth_results.dkim.clone(),
                dmarc_result: dmarc_result.to_string(),
                disposition: disposition.to_string(),
            })
            .await;
    }
}

/// Render mail-auth's ARC verdict as the coarse wire string
/// `"none" | "pass" | "fail"` (matches `ctx.auth_results.arc`).
fn arc_result_str(arc_output: &mail_auth::ArcOutput, auth_msg: &AuthenticatedMessage) -> String {
    if auth_msg.ams_headers.is_empty() {
        "none".into()
    } else if arc_output.can_be_sealed() {
        "pass".into()
    } else {
        "fail".into()
    }
}

/// Render mail-auth's DKIM coarse (any-pass) verdict as
/// `"none" | "pass" | "fail"` (matches `ctx.auth_results.dkim`).
fn dkim_result_str(dkim_outputs: &[mail_auth::DkimOutput]) -> String {
    if dkim_outputs.is_empty() {
        "none".into()
    } else if dkim_outputs
        .iter()
        .any(|o| matches!(o.result(), mail_auth::DkimResult::Pass))
    {
        "pass".into()
    } else {
        "fail".into()
    }
}

fn spf_result_str(result: mail_auth::SpfResult) -> String {
    match result {
        mail_auth::SpfResult::Pass => "pass",
        mail_auth::SpfResult::Fail => "fail",
        mail_auth::SpfResult::SoftFail => "softfail",
        mail_auth::SpfResult::Neutral => "neutral",
        mail_auth::SpfResult::None => "none",
        mail_auth::SpfResult::TempError => "temperror",
        mail_auth::SpfResult::PermError => "permerror",
    }
    .to_string()
}

#[cfg(test)]
#[path = "mail_auth_tests.rs"]
mod tests;
