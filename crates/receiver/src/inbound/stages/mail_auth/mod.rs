//! SPF + DKIM + ARC + DMARC bundled stage.
//!
//! These four checks share intermediate state (DKIM signature outputs feed
//! the DMARC alignment evaluator, SPF result + From-domain together gate
//! DMARC policy) so they run as a single stage rather than four. Writes
//! every field of `ctx.auth_results` and, on DMARC policy=reject,
//! short-circuits with a 550 reject.

use std::sync::Arc;

use async_trait::async_trait;
use hickory_resolver::TokioResolver;
use mailrs_dmarc::DmarcResultRecord;
use mailrs_inbound::{ReceiveContext, Stage, StageOutcome};

mod arc;
mod dkim;
mod dmarc;
mod spf;

use arc::run_arc;
use dkim::{dkim_result_str, run_dkim_all};
use dmarc::lookup_policy;
use spf::run_spf;

/// Bundle of DNS resolvers driving the four mailrs auth crates. One
/// hickory binding feeds all four; we wrap it in the resolver shapes
/// each crate expects so each can be tested with mocks independently.
#[derive(Clone)]
pub struct MailAuthResolvers {
    /// SPF resolver (RFC 7208 §4.6.4 / §4.6.5 lookups).
    pub spf: Arc<mailrs_spf::HickoryResolver>,
    /// DKIM resolver (TXT lookups for `<selector>._domainkey.<d>`).
    pub dkim: Arc<mailrs_dkim::HickoryDkimResolver>,
    /// ARC resolver — reuses the DKIM resolver shape since
    /// `ArcResolver = DkimResolver`.
    pub arc: Arc<mailrs_dkim::HickoryDkimResolver>,
    /// DMARC TXT-lookup resolver (`_dmarc.<from-domain>`).
    pub dmarc: Arc<TokioResolver>,
}

/// Fire-and-forget sink for per-message DMARC results feeding aggregate
/// reporting. A dyn-compatible narrowing of the stone's `DmarcStore` (which
/// carries an associated `Error` type and the report-generation methods the
/// receiver never calls) — the receiver only records; the spg-backed impl
/// lives in the core. Errors are swallowed: a lost aggregate row must never
/// block delivery.
#[async_trait]
pub trait DmarcReportSink: Send + Sync {
    /// Record one verified DMARC result for aggregate reporting.
    async fn record_result(&self, record: &DmarcResultRecord);
}

/// Stage that performs SPF, DKIM, ARC, and DMARC verification and records
/// the aggregate result in `ctx.auth_results`. On DMARC policy=reject
/// returns `Decide(Reject)`; otherwise returns `Continue`.
///
/// Built on the in-house `mailrs-spf` / `mailrs-dkim` / `mailrs-arc` /
/// `mailrs-dmarc` crates (DEPS_AUDIT #1 closed — `mail-auth` removed).
pub struct MailAuthStage {
    resolvers: MailAuthResolvers,
    dmarc_sink: Option<Arc<dyn DmarcReportSink>>,
}

impl MailAuthStage {
    /// Construct a `MailAuthStage`. The optional [`DmarcReportSink`] records
    /// per-message DMARC outcomes for aggregate reporting — injected as a
    /// trait object (the spg-backed impl is built in the core) so this stage
    /// doesn't bind the report store.
    pub fn new(resolvers: MailAuthResolvers, dmarc_sink: Option<Arc<dyn DmarcReportSink>>) -> Self {
        Self {
            resolvers,
            dmarc_sink,
        }
    }

    pub(super) fn dmarc_resolver(&self) -> &TokioResolver {
        self.resolvers.dmarc.as_ref()
    }

    pub(super) fn sink(&self) -> Option<&Arc<dyn DmarcReportSink>> {
        self.dmarc_sink.as_ref()
    }
}

/// Extract the `@<domain>` part from an RFC 5322 `From:` line.
/// Returns `None` if no `@` is present or the input is malformed.
///
/// Handles both `local@domain` and `Name <local@domain>` forms.
/// Tolerates trailing whitespace / CRLF.
pub(super) fn from_domain(from_header: &str) -> Option<String> {
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
        // 1. SPF — populates ctx.auth_results.spf, returns the typed result
        //    so we can feed an aligned SPF signal into DMARC below.
        let spf_result = run_spf(self.resolvers.spf.as_ref(), ctx).await;

        // 2. DKIM — verify every signature on the message body. Coarse
        //    pass/fail/none lands in ctx.auth_results.dkim; per-signature
        //    outputs feed DMARC alignment.
        let dkim_outputs = run_dkim_all(self.resolvers.dkim.as_ref(), &ctx.message).await;
        ctx.auth_results.dkim = dkim_result_str(&dkim_outputs);

        // 3. ARC — chain extract + full crypto verify (RFC 8617).
        ctx.auth_results.arc = run_arc(self.resolvers.arc.as_ref(), &ctx.message).await;

        // 4. DMARC — From-domain extract, _dmarc TXT lookup, alignment +
        //    evaluation against SPF/DKIM verdicts above. apply_dmarc_policy
        //    enforces p=reject / p=quarantine, records aggregate row.
        let mail_from_domain = ctx
            .sender
            .rsplit_once('@')
            .map(|(_, d)| d.to_string())
            .unwrap_or_else(|| ctx.ehlo_domain.clone());

        let from_dom = mailrs_rfc5322::Message::new(&ctx.message)
            .header_str("From")
            .and_then(from_domain);
        let Some(from_d) = from_dom else {
            // No parseable From — DMARC can't anchor; leave as default
            // ("none") and continue. mail-auth would do the same.
            return StageOutcome::Continue;
        };

        let policy = match lookup_policy(self.dmarc_resolver(), &from_d).await {
            Some(p) => p,
            None => {
                // Either no _dmarc TXT or unparseable; both behave like
                // p=none. Leave defaults; continue.
                return StageOutcome::Continue;
            }
        };

        let spf_input =
            (spf_result == mailrs_spf::SpfResult::Pass).then(|| mailrs_dmarc::SpfResult {
                domain: mail_from_domain.clone().into(),
                pass: true,
            });
        let dkim_input = dkim_outputs
            .iter()
            .filter_map(|o| {
                let d = o.domain();
                if d.is_empty() {
                    None
                } else {
                    Some(mailrs_dmarc::DkimSignatureResult {
                        d_domain: d.into(),
                        pass: o.is_pass(),
                    })
                }
            })
            .collect::<Vec<_>>();
        let input = mailrs_dmarc::DmarcInput {
            from_domain: from_d.clone().into(),
            policy_domain: from_d.into(),
            spf: spf_input,
            dkim: dkim_input,
        };
        let outcome = mailrs_dmarc::evaluate(&policy, &input);

        self.apply_dmarc_policy(ctx, &outcome, &mail_from_domain)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::spf::spf_result_str;
    use mailrs_spf::SpfResult;

    #[test]
    fn spf_result_str_covers_all_variants() {
        assert_eq!(spf_result_str(&SpfResult::Pass), "pass");
        assert_eq!(spf_result_str(&SpfResult::Fail), "fail");
        assert_eq!(spf_result_str(&SpfResult::SoftFail), "softfail");
        assert_eq!(spf_result_str(&SpfResult::Neutral), "neutral");
        assert_eq!(spf_result_str(&SpfResult::None), "none");
        assert_eq!(spf_result_str(&SpfResult::TempError), "temperror");
        assert_eq!(spf_result_str(&SpfResult::PermError), "permerror");
    }
}
