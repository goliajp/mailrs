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
}

pub(crate) fn disposition(holds: bool, currently_held: bool) -> Disposition {
    match (holds, currently_held) {
        (true, _) => Disposition::Act,
        (false, true) => Disposition::Release,
        (false, false) => Disposition::Leave,
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct RescanQuery {
    /// Report without moving anything. **Default true.**
    #[serde(default = "yes")]
    pub(super) dry_run: bool,
    #[serde(default)]
    pub(super) skip: u64,
    #[serde(default = "default_limit")]
    pub(super) limit: u64,
    #[serde(default = "default_pause_ms")]
    pub(super) pause_ms: u64,
    /// `junk` (default) or `delete`.
    #[serde(default)]
    pub(super) action: Action,
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
