//! DKIM: verify-all via `mailrs-dkim` (RFC 6376) + coarse result string.

use mailrs_dkim::{HickoryDkimResolver, SignatureOutput, verify_all};

/// Verify every DKIM-Signature on the message body via `mailrs-dkim`
/// and return the per-signature outputs. Caller renders the coarse
/// pass/fail/none string via [`dkim_result_str`] and feeds the
/// per-signature `domain()` / `is_pass()` into DMARC alignment.
pub(super) async fn run_dkim_all(
    resolver: &HickoryDkimResolver,
    raw_message: &[u8],
) -> Vec<SignatureOutput> {
    verify_all(resolver, raw_message).await
}

/// Coarse DKIM verdict: `"pass"` if any signature verified, `"fail"`
/// if any signature was present but none verified, `"none"` if the
/// message had no DKIM signatures.
pub(super) fn dkim_result_str(outputs: &[SignatureOutput]) -> String {
    if outputs.is_empty() {
        "none".into()
    } else if outputs.iter().any(|o| o.is_pass()) {
        "pass".into()
    } else {
        "fail".into()
    }
}
