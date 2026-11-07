//! The third ground: the reported message, judged again when it is
//! reported, is held by a hold rule of the rule set in force.
//!
//! The verdict stored when the message arrived is not read: the rule that
//! made it may since have been removed, moved to shadow, or made to score
//! only. A shadow rule's hits are never findings, and a rule that only
//! scores does not hold, so neither is a ground. A message that cannot be
//! judged (its file is gone, or the rules fail) is not held by a rule.

use mailrs_fraud::Policy;
use mailrs_fraud_lua::stored::relation::Counted;

use super::ReportStore;

/// What judging a stored message reads besides its file.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub policy: Policy,
    /// This deployment's `authserv-id` (`MAILRS_HOSTNAME`).
    pub our_stamp: Option<String>,
    /// The text read out of the message's attachments.
    pub attachment_texts: Vec<String>,
    /// The web addresses its QR codes carry. Always empty on the SQL core,
    /// which does not read images for them.
    pub qr_urls: Vec<String>,
}

/// The hold rule that made the third ground, and the rule set it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldRule {
    pub rule: String,
    pub version: String,
}

/// The policy the receiver judges with, from the same variables, with this
/// core's account display names.
pub fn policy(account_names: Vec<String>) -> Policy {
    Policy {
        org_names: csv_env("MAILRS_ORG_NAMES"),
        our_domains: csv_env("MAILRS_LOCAL_DOMAINS"),
        allowed_domains: csv_env("MAILRS_ORG_NAME_ALLOWED_DOMAINS"),
        account_names,
    }
}

fn csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The first hold rule the rules in force find in `user`'s copy of `mid`.
pub(super) fn in_force<S: ReportStore + ?Sized>(
    core: &S,
    user: &str,
    mid: &str,
    raw: Option<&[u8]>,
) -> Option<HoldRule> {
    let Some(raw) = raw else {
        tracing::warn!(%mid, "report: the message's file is gone, so no rule can hold it");
        return None;
    };
    let inputs = core.fraud_inputs(mid);
    let mut hist = core.net_conn();
    let judged = mailrs_fraud_lua::stored::judge_with(
        raw,
        Some((user, Counted::After)),
        inputs.our_stamp.as_deref(),
        mailrs_fraud_lua::stored::Extracted {
            texts: &inputs.attachment_texts,
            qr_urls: &inputs.qr_urls,
        },
        &inputs.policy,
        &mut hist,
        &mut |facts, policy| core.scan(facts, policy),
    );
    let judged = match judged {
        Ok(j) => j,
        Err(e) => {
            tracing::error!(%mid, error = %e, "report: the fraud rules could not run");
            return None;
        }
    };
    let rule = judged.findings.iter().find(|f| f.holds)?;
    Some(HoldRule {
        rule: rule.rule.clone(),
        version: judged.verdict.rules_version,
    })
}
