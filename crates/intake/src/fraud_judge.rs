//! Stored mail through the fraud rules, the same in every store: what the
//! rules make of a message now, and what that calls for given whether its
//! conversation is held. Used by the re-judge after a reputation change
//! and by the sweeps that re-read stored mail.

use mailrs_fraud::{Findings, Policy};
use mailrs_inbound::FraudVerdict;

/// The rules and what they read besides the message: the fraud policy,
/// this deployment's `authserv-id`, and the shared store the reader's
/// history with a sender is read from.
pub struct FraudJudge {
    policy: Policy,
    our_stamp: Option<String>,
    hist: Option<kevy_client::Connection>,
}

/// What the fraud rules make of one message.
pub struct FraudJudgement {
    pub findings: Findings,
    /// The rules would hold it.
    pub holds: bool,
    /// Not held, but scored into Junk.
    pub junks: bool,
    pub version: String,
    /// The verdict stored beside the message when acting on this judgement.
    pub verdict: FraudVerdict,
}

/// The text read out of a message's attachments, and the web addresses
/// its QR codes carry.
#[derive(Default)]
pub struct Extracted {
    pub texts: Vec<String>,
    pub qr_urls: Vec<String>,
}

impl FraudJudge {
    pub fn new(
        policy: Policy,
        our_stamp: Option<String>,
        hist: Option<kevy_client::Connection>,
    ) -> Self {
        Self {
            policy,
            our_stamp,
            hist,
        }
    }

    /// Judge `raw` as `user`'s copy: the reader's history with the sender
    /// (the BEC rules) and whether the sender's and links' domains were new
    /// when the message arrived are part of what the rules see.
    ///
    /// The history is read as it stands now, with this message already
    /// counted (`Counted::After`): "first contact" means "the only mail
    /// from this sender so far", not "the first at the time", which a
    /// stored message cannot know.
    pub fn judge(
        &mut self,
        user: &str,
        raw: &[u8],
        extracted: &Extracted,
    ) -> Result<FraudJudgement, String> {
        let j = mailrs_fraud_lua::stored::judge_for(
            raw,
            Some((user, mailrs_fraud_lua::stored::relation::Counted::After)),
            self.our_stamp.as_deref(),
            mailrs_fraud_lua::stored::Extracted {
                texts: &extracted.texts,
                qr_urls: &extracted.qr_urls,
            },
            &self.policy,
            &mut self.hist,
        )?;
        Ok(FraudJudgement {
            junks: !j.holds && junks_on_its_own(&j.findings),
            holds: j.holds,
            version: j.verdict.rules_version.clone(),
            verdict: j.verdict,
            findings: j.findings,
        })
    }
}

/// What a judged conversation calls for.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Disposition {
    /// The findings hold.
    Act,
    /// Nothing holds it and it is not held. Leave it alone.
    Leave,
    /// Nothing holds it any more, and it is still hidden: this is what
    /// lets a rule change reach mail that is already hidden.
    Release,
    /// Nothing holds it, but the fraud score alone reaches the Junk
    /// threshold, the same sum that sends it to Junk on the receive path.
    /// Junk only: a score is suspicion, and suspicion earns no hold.
    Junk,
}

pub fn disposition(holds: bool, junks: bool, currently_held: bool) -> Disposition {
    match (holds, currently_held, junks) {
        (true, _, _) => Disposition::Act,
        (false, true, _) => Disposition::Release,
        (false, false, true) => Disposition::Junk,
        (false, false, false) => Disposition::Leave,
    }
}

/// Whether the fraud findings alone would send a message to Junk on the
/// receive path. The threshold is the default: the receiver's
/// `MAILRS_SPAM_SCORE_THRESHOLD` is set nowhere in production.
pub fn junks_on_its_own(findings: &Findings) -> bool {
    findings.score() >= mailrs_inbound::DEFAULT_SPAM_THRESHOLD
}
