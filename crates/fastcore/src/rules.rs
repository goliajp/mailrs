//! A rule's actions, done to one conversation as its mail arrives.
//!
//! The state changes only: a rule is the user's standing order, not a
//! judgement made on this message, so it trains nothing and records no
//! engagement — those learn from what a person does by hand.

use std::sync::Arc;

use mailrs_mailbox_kevy::rules::Action;

use crate::FastcoreState;

pub(crate) fn apply(state: &Arc<FastcoreState>, user: &str, thread_id: &str, actions: &[Action]) {
    for action in actions {
        let done = match action {
            Action::Label(id) => state
                .mailbox
                .set_label(user, id, thread_id, true)
                .map(|_| true)
                .map_err(|e| format!("{e:?}")),
            Action::Archive => state
                .mailbox
                .set_archived(user, thread_id, true)
                .inspect(|ok| {
                    if *ok {
                        crate::keywords::set_on_thread(
                            state,
                            user,
                            thread_id,
                            crate::keywords::ARCHIVED,
                            true,
                        );
                    }
                })
                .map_err(|e| e.to_string()),
            Action::Read => Ok(crate::routes::thread_actions::mark_thread_read_everywhere(
                state, user, thread_id,
            )),
            Action::Star => state
                .mailbox
                .set_starred(user, thread_id, true)
                .map_err(|e| e.to_string()),
            Action::Junk => state
                .mailbox
                .set_junk(user, thread_id, true)
                .map_err(|e| e.to_string()),
            Action::Trash => state
                .mailbox
                .set_trashed(user, thread_id, true, chrono::Utc::now().timestamp())
                .map_err(|e| e.to_string()),
        };
        if let Err(e) = done {
            tracing::warn!(error = %e, %user, %thread_id, action = %action.spelling(), "rule action failed");
        }
    }
}
