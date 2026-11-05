//! Messages the rules hold that a round left unheld, and which round.
//!
//! Such a message's verdict already reads "hold", so a later round would
//! find nothing changed and pass it by. The mark makes a later, different
//! observation offer it again; the round that left it never does.

use crate::FastcoreState;

fn key(user: &str, mid: &str) -> String {
    format!("rejudge:unheld:{user}:{mid}")
}

/// Mark `mid` as left unheld by `observation`'s round.
pub(crate) fn leave_unheld(
    state: &FastcoreState,
    user: &str,
    mid: &str,
    observation: &str,
) -> Result<(), String> {
    state
        .mailbox
        .store_ref()
        .set(key(user, mid).as_bytes(), observation.as_bytes())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Whether a round other than `observation`'s left `mid` unheld, so that
/// `observation` offers it again although its verdict did not change.
pub(crate) fn unheld_by_another(
    state: &FastcoreState,
    user: &str,
    mid: &str,
    observation: &str,
) -> bool {
    state
        .mailbox
        .store_ref()
        .get(key(user, mid).as_bytes())
        .ok()
        .flatten()
        .is_some_and(|o| o != observation.as_bytes())
}

/// `mid` was held: nothing left it unheld any more.
pub(super) fn taken(state: &FastcoreState, user: &str, mid: &str) -> Result<(), String> {
    state
        .mailbox
        .store_ref()
        .del(&[key(user, mid).as_bytes()])
        .map(|_| ())
        .map_err(|e| e.to_string())
}
