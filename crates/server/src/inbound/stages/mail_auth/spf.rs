//! SPF: production run via `mailrs-spf` (RFC 7208) + result-string mapping.

use mailrs_inbound::ReceiveContext;
use mailrs_spf::{HickoryResolver, SpfResult, VerifyInput, verify};

/// Run `mailrs-spf` against the envelope and stash the coarse wire-form
/// result into `ctx.auth_results.spf`. Returns the typed `SpfResult`
/// so the caller can feed an aligned-SPF signal into DMARC.
pub(super) async fn run_spf(resolver: &HickoryResolver, ctx: &mut ReceiveContext) -> SpfResult {
    let input = VerifyInput {
        ip: ctx.client_ip,
        helo: ctx.ehlo_domain.clone(),
        mail_from: ctx.sender.clone(),
    };
    let result = verify(resolver, &input).await;
    ctx.auth_results.spf = spf_result_str(&result);
    result
}

/// Render `SpfResult` as the lowercase wire form per RFC 7001 §2.7.2.
pub(super) fn spf_result_str(result: &SpfResult) -> String {
    result.as_str().to_string()
}
