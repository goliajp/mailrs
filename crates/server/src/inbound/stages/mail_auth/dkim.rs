//! DKIM: shadow check + DKIM-result string mapping.

use mailrs_inbound::ReceiveContext;

use super::MailAuthStage;

impl MailAuthStage {
    /// Run `mailrs_dkim::verify_all` against the raw message; log
    /// match/divergence vs mail-auth's coarse DKIM verdict; return
    /// the per-signature outputs so the DMARC shadow check can
    /// align by `d=` domain.
    pub(super) async fn shadow_check_dkim(
        &self,
        ctx: &ReceiveContext,
    ) -> Vec<mailrs_dkim::SignatureOutput> {
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
}

pub(super) fn dkim_result_str(dkim_outputs: &[mail_auth::DkimOutput]) -> String {
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
