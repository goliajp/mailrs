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
use mail_auth::{AuthenticatedMessage, MessageAuthenticator};
use mailrs_inbound::{ReceiveContext, Stage, StageOutcome};

use crate::dmarc_report::DmarcReportStore;

mod arc;
mod dkim;
mod dmarc;
mod spf;

use arc::arc_result_str;
use dkim::dkim_result_str;

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

#[cfg(test)]
mod tests {
    use super::spf::spf_result_str;

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

