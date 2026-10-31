//! Outgoing mail through the fraud rules, for fastcore-sender: the same
//! reading of a message the re-scan sweep uses on stored mail.

/// The findings for one outgoing message, and the rule version.
///
/// # Errors
/// The rules could not run.
pub fn judge(raw: &[u8]) -> Result<mailrs_fraud_lua::Scan, String> {
    crate::maintenance::fraud_rescan::judge_outgoing(raw)
}
