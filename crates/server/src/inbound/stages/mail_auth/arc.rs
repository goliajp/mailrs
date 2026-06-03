//! ARC: chain extract + full crypto verify via `mailrs-arc` (RFC 8617).

use mailrs_arc::{ArcChain, ChainOutcome, verify_chain_with_crypto};
use mailrs_dkim::HickoryDkimResolver;

/// Verify the ARC chain (if any) on the raw message and return the
/// coarse wire string `"none" | "pass" | "fail"` (matches the shape
/// `ctx.auth_results.arc` expects).
pub(super) async fn run_arc(resolver: &HickoryDkimResolver, raw_message: &[u8]) -> String {
    let chain = match ArcChain::extract(raw_message) {
        Ok(None) => return "none".into(),
        Err(_) => return "fail".into(),
        Ok(Some(c)) => c,
    };
    match verify_chain_with_crypto(&chain, resolver, raw_message).await {
        Ok(ChainOutcome::Pass) => "pass".into(),
        _ => "fail".into(),
    }
}
