//! A reader's junk marks and releases, as the fraud rules read them.
//!
//! Marking not junk mail from a sender whose site lands on another domain
//! releases that company from the burner-domain rule for the reader,
//! marking junk takes it back
//! ([`mailrs_core_sidestate::families::burner_released`]). Releasing a
//! held conversation lets its senders through the display-name rule for
//! the reader, and a junk mark takes that back too
//! ([`mailrs_core_sidestate::families::contacts::release`]).

use std::sync::Arc;

use crate::WebState;
use mailrs_core_sidestate::families::{burner_released, contacts};

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
    let addresses = others(&user, senders_csv);
    let hosts: Vec<String> = addresses
        .iter()
        .filter_map(|a| a.rsplit_once('@').map(|(_, h)| h.to_string()))
        .collect();
    if hosts.is_empty() {
        return;
    }
    let done = crate::handlers::kevy_util::with_kevy(move |c| {
        if !not_junk {
            for address in &addresses {
                contacts::unrelease(c, &user, address)?;
            }
        }
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

/// The thread's senders other than the reader, lowercased.
fn others(user: &str, senders_csv: &str) -> Vec<String> {
    mailrs_core_api::types::sender_addresses(senders_csv)
        .into_iter()
        .filter(|a| a != user)
        .collect()
}

/// `user` released the held conversation `thread_id`: its senders are let
/// through the display-name rule for them from now on. Bookkeeping beside
/// the release, which has already happened: a store that cannot be
/// reached is logged.
pub(crate) async fn released_from_hold(state: &Arc<WebState>, user: &str, thread_id: &str) {
    let user = user.to_ascii_lowercase();
    let addresses = others(&user, &thread_senders(state, &user, thread_id).await);
    if addresses.is_empty() {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let done = crate::handlers::kevy_util::with_kevy(move |c| {
        for address in &addresses {
            contacts::release(c, &user, address, now)?;
        }
        tracing::info!(event = "hold_release_sender", %user, senders = ?addresses, "held senders released");
        Ok(())
    });
    if let Err(status) = done {
        tracing::warn!(?status, "held senders not released");
    }
}
