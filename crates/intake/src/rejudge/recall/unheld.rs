//! Messages the rules hold that a round left unheld, and which round.
//!
//! Such a message's verdict already reads "hold", so a later round would
//! find nothing changed and pass it by. The mark makes a later, different
//! observation offer it again; the round that left it never does.

use super::RejudgeStore;

fn key(user: &str, mid: &str) -> String {
    format!("rejudge:unheld:{user}:{mid}")
}

/// Mark `mid` as left unheld by `observation`'s round.
pub fn leave_unheld<S: RejudgeStore>(
    store: &S,
    user: &str,
    mid: &str,
    observation: &str,
) -> Result<(), String> {
    store
        .state_set(&key(user, mid), observation.as_bytes())
        .map_err(|e| e.to_string())
}

/// Whether a round other than `observation`'s left `mid` unheld, so that
/// `observation` offers it again although its verdict did not change.
pub fn unheld_by_another<S: RejudgeStore>(
    store: &S,
    user: &str,
    mid: &str,
    observation: &str,
) -> bool {
    store
        .state_get(&key(user, mid))
        .ok()
        .flatten()
        .is_some_and(|o| o != observation.as_bytes())
}

/// `mid` was held: nothing left it unheld any more.
pub(super) fn taken<S: RejudgeStore>(store: &S, user: &str, mid: &str) -> Result<(), String> {
    store.state_del(&key(user, mid)).map_err(|e| e.to_string())
}
