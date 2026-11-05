//! The answer a sweep gives: its counts and samples, as JSON.

use super::*;

impl Tally {
    pub(super) fn into_json(
        self,
        q: &RescanQuery,
        walked: u64,
        stopped_early: bool,
    ) -> serde_json::Value {
        tracing::info!(
            walked,
            found = self.found,
            moved = self.moved,
            deleted = self.deleted,
            held = self.held,
            verdict_failed = self.verdict_failed,
            releasable = self.releasable,
            released = self.released,
            kept_released = self.kept_released,
            held_but_unreadable = self.held_but_unreadable,
            already_junk = self.already_junk,
            no_file = self.no_file,
            dry_run = q.dry_run,
            "fraud-rescan complete"
        );
        serde_json::json!({
            "done": !stopped_early,
            "next_skip": q.skip + walked,
            "dry_run": q.dry_run,
            "threads_walked": walked,
            "found": self.found,
            "moved_to_junk": self.moved,
            "deleted": self.deleted,
            "held": self.held,
            // Held but unable to explain itself. Reported rather than
            // folded into `held`, because a hold nobody can see the
            // reasons for is the one that turns into "my mail vanished".
            "verdict_failed": self.verdict_failed,
            // Held by a rule that no longer holds anything. Two numbers
            // rather than one: a dry run can only report `releasable`,
            // and a single figure of 0 would not say whether it found
            // none or was not allowed to act.
            "releasable": self.releasable,
            "released": self.released,
            // Hold-grade, but a person released the conversation after
            // this message came: left where they put it.
            "kept_released": self.kept_released,
            // Hidden, and its file is gone: it cannot be re-judged, so
            // no rule change will ever let it out. Zero is the answer
            // that means what it says.
            "held_but_unreadable": self.held_but_unreadable,
            "release_samples": self.release_samples,
            "already_junk": self.already_junk,
            "no_file": self.no_file,
            "by_reason": self.by_reason,
            "samples": self.samples,
            "scored_found": self.scored_found,
            "scored_moved_to_junk": self.scored_moved,
            "scored_already_junk": self.scored_already_junk,
            "scored_but_whitelisted": self.scored_whitelisted,
            "scored_by_reason": self.scored_by_reason,
            "scored_samples": self.scored_samples,
            // what the walk changes against the stored state; in a dry run,
            // what applying it would change
            "from_stored": self.from_stored.json(),
        })
    }
}
