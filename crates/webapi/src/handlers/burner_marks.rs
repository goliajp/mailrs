//! A reader's junk marks, as the burner-domain rule reads them: marking
//! not junk mail from a sender whose site lands on another domain releases
//! that company for the reader, marking junk takes it back
//! ([`mailrs_core_sidestate::families::burner_released`]).

use std::sync::Arc;

use crate::WebState;
use mailrs_core_sidestate::families::burner_released;

/// The thread's participants as the core lists them, `""` when it cannot
/// say.
pub(crate) async fn thread_senders(state: &Arc<WebState>, user: &str, thread_id: &str) -> String {
    let req = mailrs_core_api::method::conversation::ConversationsByIdsRequest {
        folder: None,
        thread_ids: vec![thread_id.to_string()],
    };
    match state.core.conversations_by_thread_ids(user, &req).await {
        Ok(resp) => resp
            .items
            .into_iter()
            .next()
            .map(|c| c.participants)
            .unwrap_or_default(),
        Err(e) => {
            tracing::warn!(err = ?e, %user, %thread_id, "thread lookup failed; burner marks not updated");
            String::new()
        }
    }
}

/// Record `user`'s mark on mail from `senders_csv` (the thread's
/// participants): `not_junk` releases each sender's company, otherwise
/// the release is taken back. Bookkeeping beside the move, which is the
/// load-bearing part: a store that cannot be reached is logged.
pub(crate) fn record(user: &str, senders_csv: &str, not_junk: bool) {
    let user = user.to_ascii_lowercase();
    let hosts: Vec<String> = mailrs_core_api::types::sender_addresses(senders_csv)
        .into_iter()
        .filter(|a| *a != user)
        .filter_map(|a| a.rsplit_once('@').map(|(_, h)| h.to_string()))
        .collect();
    if hosts.is_empty() {
        return;
    }
    let done = crate::handlers::kevy_util::with_kevy(move |c| {
        for host in &hosts {
            let changed = match not_junk {
                true => burner_released::released(c, &user, host)?,
                false => burner_released::junked(c, &user, host)?,
            };
            if !changed.is_empty() {
                tracing::info!(event = "burner_mark", %user, %host, not_junk, domains = ?changed, "burner-domain release updated");
            }
        }
        Ok(())
    });
    if let Err(status) = done {
        tracing::warn!(?status, "burner marks not updated");
    }
}
