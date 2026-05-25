//! ARC: shadow check + ARC-result string mapping.

use mail_auth::AuthenticatedMessage;

use mailrs_inbound::ReceiveContext;

use super::MailAuthStage;

impl MailAuthStage {
    /// the raw message; log match/divergence with mail-auth's ARC
    /// result already in `ctx.auth_results.arc`.
    pub(super) async fn shadow_check_arc(&self, ctx: &ReceiveContext) {
        let Some(ref shadow) = self.shadow_arc_resolver else {
            return;
        };
        let shadow_coarse = match mailrs_arc::ArcChain::extract(&ctx.message) {
            Ok(None) => "none",
            Err(_) => "fail",
            Ok(Some(chain)) => {
                match mailrs_arc::verify_chain_with_crypto(&chain, shadow.as_ref(), &ctx.message)
                    .await
                {
                    Ok(mailrs_arc::ChainOutcome::Pass) => "pass",
                    Ok(_) | Err(_) => "fail",
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
}

/// Render mail-auth's ARC verdict as the coarse wire string
/// `"none" | "pass" | "fail"` (matches `ctx.auth_results.arc`).
pub(super) fn arc_result_str(
    arc_output: &mail_auth::ArcOutput,
    auth_msg: &AuthenticatedMessage,
) -> String {
    if auth_msg.ams_headers.is_empty() {
        "none".into()
    } else if arc_output.can_be_sealed() {
        "pass".into()
    } else {
        "fail".into()
    }
}
