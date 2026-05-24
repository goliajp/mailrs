//! DMARC: shadow check + production policy + recording.


use mailrs_inbound::{DeliveryDecision, DmarcPolicy, ReceiveContext, StageOutcome};

use crate::dmarc_report::{DmarcResultRecord, DmarcStore};

use super::{from_domain, MailAuthStage};

impl MailAuthStage {
    /// `DmarcInput` from the SPF / per-sig DKIM signals already
    /// computed, evaluate via mailrs-dmarc, and log match/divergence
    /// vs mail-auth's coarse DMARC verdict.
    pub(super) async fn shadow_check_dmarc(
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


    /// "pass"`; reject short-circuits with a 5.7.1 reject (and
    /// records the result for aggregate reporting); quarantine /
    /// none label the message and let it through. All non-reject
    /// outcomes also record the result for aggregate reports.
    pub(super) async fn apply_dmarc_policy(
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
    pub(super) async fn record_dmarc(
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
