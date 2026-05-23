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
}

#[async_trait]
impl Stage for MailAuthStage {
    fn name(&self) -> &str {
        "mail_auth"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        // 1. SPF
        let spf_params = SpfParameters::verify_mail_from(
            ctx.client_ip,
            &ctx.ehlo_domain,
            &ctx.hostname,
            &ctx.sender,
        );
        let spf_output = self.authenticator.verify_spf(spf_params).await;
        let spf_str = spf_result_str(spf_output.result());
        ctx.auth_results.spf = spf_str.clone();

        // Shadow validation against mailrs-spf — runs only when the
        // optional resolver is configured. Does NOT affect any decision;
        // we log matches at info, divergences at warn so we can audit.
        if let Some(ref shadow) = self.shadow_spf_resolver {
            let input = mailrs_spf::VerifyInput {
                ip: ctx.client_ip,
                helo: ctx.ehlo_domain.clone(),
                mail_from: ctx.sender.clone(),
            };
            let shadow_result = mailrs_spf::verify(shadow.as_ref(), &input).await;
            let shadow_str = shadow_result.as_str();
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

        // 2. DKIM + 3. ARC + 4. DMARC (need parsed message)
        let Some(auth_msg) = AuthenticatedMessage::parse(&ctx.message) else {
            return StageOutcome::Continue;
        };

        let dkim_outputs = self.authenticator.verify_dkim(&auth_msg).await;
        let arc_output = self.authenticator.verify_arc(&auth_msg).await;

        ctx.auth_results.arc = if auth_msg.ams_headers.is_empty() {
            "none".into()
        } else if arc_output.can_be_sealed() {
            "pass".into()
        } else {
            "fail".into()
        };

        // Shadow-mode ARC via mailrs-arc 1.1. Reads the same raw
        // bytes mail-auth read, runs structural + crypto verify
        // through its own DKIM-shaped resolver. Logs match /
        // divergence; does NOT affect any decision.
        if let Some(ref shadow) = self.shadow_arc_resolver {
            let shadow_coarse =
                match mailrs_arc::ArcChain::extract(&ctx.message) {
                    Ok(None) => "none",
                    Err(_) => "fail",
                    Ok(Some(chain)) => {
                        match mailrs_arc::verify_chain_with_crypto(
                            &chain,
                            shadow.as_ref(),
                            &ctx.message,
                        )
                        .await
                        {
                            Ok(mailrs_arc::ChainOutcome::Pass) => "pass",
                            Ok(_) => "fail",
                            Err(_) => "fail",
                        }
                    }
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

        ctx.auth_results.dkim = if dkim_outputs.is_empty() {
            "none".into()
        } else if dkim_outputs
            .iter()
            .any(|o| matches!(o.result(), mail_auth::DkimResult::Pass))
        {
            "pass".into()
        } else {
            "fail".into()
        };

        // Shadow validation against mailrs-dkim — same pattern as the
        // SPF shadow path above. Runs only when the optional resolver
        // is configured; DOES NOT affect any decision.
        if let Some(ref shadow) = self.shadow_dkim_resolver {
            let shadow_result = mailrs_dkim::verify(shadow.as_ref(), &ctx.message).await;
            // Map both to a coarse string for comparison: pass/fail/none.
            // (mailrs-dkim returns 7 values; we collapse to 3 to match
            // the mail-auth-derived ctx.auth_results.dkim field.)
            let shadow_coarse = match shadow_result {
                mailrs_dkim::DkimResult::Pass => "pass",
                mailrs_dkim::DkimResult::None => "none",
                _ => "fail",
            };
            let mail_auth_coarse = ctx.auth_results.dkim.as_str();
            if shadow_coarse == mail_auth_coarse {
                tracing::info!(
                    event = "dkim_shadow_match",
                    dkim = %mail_auth_coarse,
                    "mailrs-dkim matches mail-auth"
                );
            } else {
                tracing::warn!(
                    event = "dkim_shadow_divergence",
                    mail_auth = %mail_auth_coarse,
                    mailrs_dkim = %shadow_coarse,
                    mailrs_dkim_detail = %shadow_result,
                    sender = %ctx.sender,
                    "DKIM result divergence — mailrs-dkim says different from mail-auth"
                );
            }
        }

        let mail_from_domain = ctx
            .sender
            .rsplit_once('@')
            .map(|(_, d)| d)
            .unwrap_or(&ctx.ehlo_domain);
        let dmarc_params =
            DmarcParameters::new(&auth_msg, &dkim_outputs, mail_from_domain, &spf_output);
        let dmarc_output = self.authenticator.verify_dmarc(dmarc_params).await;

        let dmarc_pass = dmarc_output.dkim_result() == &mail_auth::DmarcResult::Pass
            || dmarc_output.spf_result() == &mail_auth::DmarcResult::Pass;
        let mut dmarc_quarantine = false;

        if dmarc_pass {
            ctx.auth_results.dmarc = "pass".into();
            ctx.auth_results.dmarc_policy = DmarcPolicy::Pass;
        } else {
            match dmarc_output.policy() {
                mail_auth::dmarc::Policy::Reject => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::Reject;

                    if let Some(store) = &self.dmarc_report_store {
                        let _ = store
                            .record_result(&DmarcResultRecord {
                                source_ip: ctx.client_ip.to_string(),
                                from_domain: mail_from_domain.to_string(),
                                spf_result: ctx.auth_results.spf.clone(),
                                dkim_result: ctx.auth_results.dkim.clone(),
                                dmarc_result: "fail".to_string(),
                                disposition: "reject".to_string(),
                            })
                            .await;
                    }

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

        // record DMARC result for aggregate reporting (non-reject paths)
        if let Some(store) = &self.dmarc_report_store {
            let disposition = if dmarc_quarantine {
                "quarantine"
            } else {
                "none"
            };
            let _ = store
                .record_result(&DmarcResultRecord {
                    source_ip: ctx.client_ip.to_string(),
                    from_domain: mail_from_domain.to_string(),
                    spf_result: ctx.auth_results.spf.clone(),
                    dkim_result: ctx.auth_results.dkim.clone(),
                    dmarc_result: ctx.auth_results.dmarc.clone(),
                    disposition: disposition.to_string(),
                })
                .await;
        }

        StageOutcome::Continue
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
mod tests {
    use super::*;

    #[test]
    fn spf_result_str_covers_all_variants() {
        assert_eq!(spf_result_str(mail_auth::SpfResult::Pass), "pass");
        assert_eq!(spf_result_str(mail_auth::SpfResult::Fail), "fail");
        assert_eq!(spf_result_str(mail_auth::SpfResult::SoftFail), "softfail");
        assert_eq!(spf_result_str(mail_auth::SpfResult::Neutral), "neutral");
        assert_eq!(spf_result_str(mail_auth::SpfResult::None), "none");
        assert_eq!(spf_result_str(mail_auth::SpfResult::TempError), "temperror");
        assert_eq!(spf_result_str(mail_auth::SpfResult::PermError), "permerror");
    }
}
