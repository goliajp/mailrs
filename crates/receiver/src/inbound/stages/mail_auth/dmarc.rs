//! DMARC: policy TXT lookup + policy enforcement + aggregate recording.

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use mailrs_dmarc::{DmarcOutcome, DmarcPolicy as MailrsPolicy, PolicyAction};
use mailrs_inbound::{DeliveryDecision, DmarcPolicy, ReceiveContext, StageOutcome};

use super::MailAuthStage;

/// Look up `_dmarc.<domain>` TXT records via the hickory resolver and
/// parse the result as a [`MailrsPolicy`]. Returns `None` when no
/// record exists or it doesn't parse — both behave like `p=none`.
pub(super) async fn lookup_policy(
    resolver: &TokioResolver,
    from_domain: &str,
) -> Option<MailrsPolicy> {
    let q = format!("_dmarc.{from_domain}");
    let lookup = resolver.txt_lookup(q).await.ok()?;
    let txt: String = lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::TXT(t) => Some(t.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    MailrsPolicy::parse(&txt).ok()
}

impl MailAuthStage {
    /// Apply the DMARC policy from `outcome` to the receive context.
    /// `pass` → label and continue; `reject` → short-circuit with 5.7.1
    /// reject (and record the result); `quarantine` / `none` → label
    /// and continue. All non-pass outcomes also record the result for
    /// aggregate reporting.
    pub(super) async fn apply_dmarc_policy(
        &self,
        ctx: &mut ReceiveContext,
        outcome: &DmarcOutcome,
        mail_from_domain: &str,
    ) -> StageOutcome {
        if outcome.dmarc_pass {
            ctx.auth_results.dmarc = "pass".into();
            ctx.auth_results.dmarc_policy = DmarcPolicy::Pass;
        } else {
            match outcome.disposition {
                PolicyAction::Reject => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::Reject;

                    tracing::info!(
                        event = "dmarc_reject",
                        domain = mail_from_domain,
                        spf = %ctx.auth_results.spf,
                        dkim = %ctx.auth_results.dkim,
                        reason = %outcome.reason,
                        "DMARC reject"
                    );

                    // an allowed sender is accepted all the same, by the final decision
                    if ctx.sender_allowed {
                        return StageOutcome::Continue;
                    }
                    return StageOutcome::Decide(DeliveryDecision::Reject {
                        code: 550,
                        message: format!("5.7.1 DMARC policy reject for domain {mail_from_domain}"),
                    });
                }
                PolicyAction::Quarantine => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::Quarantine;
                }
                PolicyAction::None => {
                    ctx.auth_results.dmarc = "fail".into();
                    ctx.auth_results.dmarc_policy = DmarcPolicy::None;
                }
            }
        }

        StageOutcome::Continue
    }
}
