//! What the sweep may do, and which of those it should.
//!
//! Split from the walk by theme: this is the decision, that is the
//! machinery around it. The release direction lives here because it
//! is the one that changes production data on the strength of a rule
//! having been edited, and it needs to be testable without a
//! `FastcoreState` the loop cannot do without.

/// What to do with a thread the checks flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Action {
    /// Move it to Junk. Reversible, and the default.
    #[default]
    Junk,
    /// Unlink its maildir files. **There is no trash and nothing to
    /// restore from** — the same warning `delete-thread-confirm` puts
    /// in front of a person.
    Delete,
    /// Hold it: out of every ordinary list, into the review screen,
    /// nothing deleted. Writes the verdict too, so the screen can say
    /// which layer convicted.
    ///
    /// The transport layer of a re-scan's verdict is read from the
    /// `Authentication-Results` header the receiver wrote at the time
    /// — the receipt, not a re-derivation. Where there is no such
    /// header the layer says so rather than claiming a pass.
    Hold,
}

/// What the sweep should do with a conversation it has judged.
///
/// A function rather than two `if`s in the loop, so the **release**
/// direction can be tested. It is the one that changes production
/// data on the strength of a rule having been edited, and the loop
/// around it needs a `FastcoreState` that a unit test cannot build.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Disposition {
    /// The findings hold. Apply the configured action.
    Act,
    /// Nothing holds it and it is not held. Leave it alone.
    Leave,
    /// Nothing holds it any more, and it is still hidden.
    ///
    /// This is what makes a rule change reach mail that is already
    /// hidden. When the two brand rules were demoted to suspicion on
    /// 2026-08-30 — familiarity with a sender is not grounds for
    /// hiding their mail — thirty conversations went on being hidden
    /// by a rule that no longer holds anything, and no sweep could
    /// have let them out: the loop reached `continue` before it ever
    /// looked at whether the thread was held.
    Release,
    /// Nothing holds it, but the fraud score alone reaches the Junk
    /// threshold — the same sum that sends it to Junk on the receive
    /// path, where `make_delivery_decision` adds `findings.score()`.
    ///
    /// Without it a scored rule reaches new mail only: the sweep acted
    /// on hold-grade findings and nothing else, so the twenty-one
    /// `subject-cut-from-the-address` messages delivered before that
    /// rule existed would have stayed in the inbox for good.
    ///
    /// Junk only, whatever `action` says. A score is suspicion, and
    /// suspicion does not earn a hold or a delete.
    Junk,
}

pub(crate) fn disposition(holds: bool, junks: bool, currently_held: bool) -> Disposition {
    match (holds, currently_held, junks) {
        (true, _, _) => Disposition::Act,
        (false, true, _) => Disposition::Release,
        (false, false, true) => Disposition::Junk,
        (false, false, false) => Disposition::Leave,
    }
}

/// Whether the fraud findings alone would send a message to Junk on
/// the receive path. The threshold is the default because fastcore is
/// not given the receiver's `MAILRS_SPAM_SCORE_THRESHOLD`, and
/// production sets it nowhere (checked 2026-09-23 with `docker
/// inspect mailrs-receiver`).
pub(crate) fn junks_on_its_own(findings: &mailrs_fraud::Findings) -> bool {
    findings.score() >= mailrs_inbound::DEFAULT_SPAM_THRESHOLD
}

#[derive(serde::Deserialize)]
pub(crate) struct RescanQuery {
    /// Report without moving anything. **Default true.**
    #[serde(default = "yes")]
    pub(crate) dry_run: bool,
    #[serde(default)]
    pub(crate) skip: u64,
    #[serde(default = "default_limit")]
    pub(crate) limit: u64,
    #[serde(default = "default_pause_ms")]
    pub(crate) pause_ms: u64,
    /// `junk` (default) or `delete`.
    #[serde(default)]
    pub(crate) action: Action,
}

fn yes() -> bool {
    true
}
fn default_limit() -> u64 {
    500
}
fn default_pause_ms() -> u64 {
    50
}
