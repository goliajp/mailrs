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

pub(crate) use mailrs_intake::fraud_judge::{Disposition, disposition, junks_on_its_own};

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
    /// The sha256 of a candidate pack, `candidate-<sha256>.lua` beside the
    /// active rules file: judge with it instead, dry, as the re-judge after
    /// activating it would (`action=hold`).
    #[serde(default)]
    pub(crate) candidate: Option<String>,
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
