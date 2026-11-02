//! Two members of a team inbox must not both answer the same message
//! without meaning to. Before a reply goes out it claims the
//! conversation: the claim compares the number of replies the team has
//! claimed with the number the member's composer saw when it opened. If
//! another member answered in between, the claim is refused and says who;
//! only a forced claim sends a second reply.
//!
//! Each store keeps one `ReplyState` per team conversation and runs
//! `claim` and `release` inside one atomic step of its own, so two
//! claims on the same seen count cannot both win.

use serde::{Deserialize, Serialize};

/// The last claimed reply, and the one before it so an undone send can be
/// taken back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyState {
    /// How many replies have been claimed.
    pub seq: u64,
    /// Who claimed the last one, lowercased.
    #[serde(default)]
    pub by: String,
    /// When, in epoch seconds.
    #[serde(default)]
    pub at: i64,
    /// What releases the last claim.
    #[serde(default)]
    pub claim_id: String,
    /// What the state was before the last claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<Box<ReplyState>>,
}

/// What a claim decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// Store `next`; the reply is the claimant's.
    Won {
        /// The state to store.
        next: ReplyState,
    },
    /// Someone else answered after what the claimant saw.
    Lost {
        /// Who answered.
        by: String,
        /// When.
        at: i64,
        /// The reply count now.
        seq: u64,
    },
}

/// Decide `user`'s claim, made at `at` with `seen_seq` replies seen.
/// The claimant's own earlier replies never stand in its way.
pub fn claim(current: &ReplyState, user: &str, seen_seq: u64, force: bool, at: i64) -> Claim {
    let user = user.trim().to_ascii_lowercase();
    let unseen = current.seq > seen_seq && current.by != user;
    if unseen && !force {
        return Claim::Lost {
            by: current.by.clone(),
            at: current.at,
            seq: current.seq,
        };
    }
    let seq = current.seq + 1;
    let mut prev = current.clone();
    prev.prev = None;
    Claim::Won {
        next: ReplyState {
            seq,
            by: user,
            at,
            claim_id: format!("{seq}-{at}"),
            prev: Some(Box::new(prev)),
        },
    }
}

/// The state after taking back `claim_id`, `None` when it is not the last
/// claim (a later reply stands, or it was already taken back).
pub fn release(current: &ReplyState, claim_id: &str) -> Option<ReplyState> {
    if current.claim_id.is_empty() || current.claim_id != claim_id {
        return None;
    }
    Some(current.prev.as_deref().cloned().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn won(c: Claim) -> ReplyState {
        match c {
            Claim::Won { next } => next,
            Claim::Lost { .. } => panic!("lost: {c:?}"),
        }
    }

    #[test]
    fn of_two_claims_on_the_same_count_only_the_first_wins() {
        let s0 = ReplyState::default();
        let a = won(claim(&s0, "a@x", 0, false, 10));
        assert_eq!(
            claim(&a, "b@x", 0, false, 11),
            Claim::Lost {
                by: "a@x".into(),
                at: 10,
                seq: 1
            }
        );
        let forced = won(claim(&a, "b@x", 0, true, 12));
        assert_eq!((forced.seq, forced.by.as_str()), (2, "b@x"));
    }

    #[test]
    fn a_member_who_saw_the_last_reply_or_wrote_it_may_answer() {
        let a = won(claim(&ReplyState::default(), "A@x", 0, false, 10));
        assert!(matches!(claim(&a, "b@x", 1, false, 11), Claim::Won { .. }));
        assert!(matches!(claim(&a, "a@x", 0, false, 11), Claim::Won { .. }));
    }

    #[test]
    fn an_undone_send_gives_the_claim_back_unless_someone_answered_since() {
        let s0 = ReplyState::default();
        let a = won(claim(&s0, "a@x", 0, false, 10));
        let back = release(&a, &a.claim_id).unwrap();
        assert_eq!(back.seq, 0);
        assert!(matches!(
            claim(&back, "b@x", 0, false, 11),
            Claim::Won { .. }
        ));
        let b = won(claim(&a, "b@x", 1, false, 12));
        assert_eq!(release(&b, &a.claim_id), None);
        assert_eq!(release(&s0, ""), None);
    }
}
