//! SPF: production run + shadow check + result-string mapping.

use mail_auth::spf::verify::SpfParameters;

use mailrs_inbound::ReceiveContext;

use super::MailAuthStage;

impl MailAuthStage {
    /// Run mail-auth's SPF verifier against the envelope and
    /// stash the coarse result into `ctx.auth_results.spf`.
    pub(super) async fn run_spf(&self, ctx: &mut ReceiveContext) -> mail_auth::SpfOutput {
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


    /// shadow resolver is configured.
    pub(super) async fn shadow_check_spf(&self, ctx: &ReceiveContext) {
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

}

pub(super) fn spf_result_str(result: mail_auth::SpfResult) -> String {
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

