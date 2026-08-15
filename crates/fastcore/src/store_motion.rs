//! Whether the store moved while a sweep was reading it.
//!
//! Phase S of `.claude/plans/nodefer-round2-2026-08-15.md`. Every
//! reconcile and shadow route walks thirty thousand threads while mail
//! arrives, so a difference it reports may be a message that landed
//! mid-walk and a zero may be one that landed behind its cursor. Round 1
//! took those zeros as proof that a repair had worked; every one of them
//! was read off a live store.
//!
//! Freezing the store was considered and refused — the design file has
//! the reasoning, and the short form is that the shadows compare the
//! **maildir** against **kevy** and a snapshot freezes only the second,
//! which manufactures differences rather than removing them.
//!
//! So a sweep reports the question instead of pretending it does not
//! exist. `changes_tail()` at entry and exit gives how many writes landed
//! while it ran.
//!
//! **A store with no feed answers `null`, not `true`.** It cannot see its
//! own writes, and letting that read as a clean sweep would be the exact
//! shape this round exists to remove: a number that cannot come out
//! dirty. `null` is the honest value for "not asked".

use std::sync::Arc;

use crate::FastcoreState;

/// The feed cursor a sweep started from, or `None` when the store has no
/// feed and the question cannot be asked.
pub(crate) struct Motion(Option<(u64, u64)>);

/// Read the cursor a sweep is starting at.
pub(crate) fn begin(state: &Arc<FastcoreState>) -> Motion {
    Motion(state.mailbox.store_ref().changes_tail().ok())
}

impl Motion {
    /// How many writes landed since [`begin`], and whether that is zero.
    ///
    /// Both are `null` in the report when the store has no feed. A
    /// generation change means the feed was reset under the sweep — a
    /// resync, a restore — which is not a count of writes and is reported
    /// as "not still" rather than as a number that would be arithmetic on
    /// two different origins.
    pub(crate) fn finish(self, state: &Arc<FastcoreState>) -> serde_json::Value {
        let Some((gen0, off0)) = self.0 else {
            return serde_json::json!({ "writes_during": null, "still": null });
        };
        let Ok((gen1, off1)) = state.mailbox.store_ref().changes_tail() else {
            return serde_json::json!({ "writes_during": null, "still": null });
        };
        if gen1 != gen0 {
            return serde_json::json!({ "writes_during": null, "still": false });
        }
        let moved = off1.saturating_sub(off0);
        serde_json::json!({ "writes_during": moved, "still": moved == 0 })
    }
}

/// Merge a motion report into a route's own response body.
///
/// The two are separate objects so a route cannot accidentally overwrite
/// one of its own fields with a name this module chose.
pub(crate) fn with_motion(
    mut body: serde_json::Value,
    motion: serde_json::Value,
) -> serde_json::Value {
    if let (Some(b), Some(m)) = (body.as_object_mut(), motion.as_object()) {
        for (k, v) in m {
            b.insert(k.clone(), v.clone());
        }
    }
    body
}

/// Drive [`begin`] / [`finish`] around `during`, for tests that need a
/// write to land between the two cursor reads.
///
/// A test seam, and deliberately the same pair the routes call: a probe
/// that measured its own copy of the logic would prove nothing about
/// theirs.
pub fn store_motion_probe(state: &Arc<FastcoreState>, during: impl FnOnce()) -> (u64, bool) {
    let started = begin(state);
    during();
    let report = started.finish(state);
    (
        report["writes_during"].as_u64().unwrap_or(0),
        report["still"].as_bool().unwrap_or(false),
    )
}
