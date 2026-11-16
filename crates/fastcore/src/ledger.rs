//! What delivery records in the fact ledger about each message.

use std::sync::Arc;
use std::time::Duration;

use crate::FastcoreState;

pub(crate) use mailrs_intake::ledger::{
    delivered_verdict, facts_of, indexed_link_hosts, is_own, judged_verdict, links_until,
    was_received,
};

/// Remove link rows past their retention, and fingerprint index rows past
/// their window, hourly; backs off to six hours
/// while there is nothing due, which costs one index read.
pub(crate) fn spawn_link_prune(state: Arc<FastcoreState>) {
    const BASE: Duration = Duration::from_secs(3600);
    const MAX: Duration = Duration::from_secs(6 * 3600);
    const BATCH: usize = 5000;
    tokio::spawn(async move {
        let mut wait = BASE;
        loop {
            tokio::time::sleep(wait).await;
            let st = state.clone();
            let pruned = tokio::task::spawn_blocking(move || {
                let now = chrono::Utc::now().timestamp();
                let mut total = 0;
                // fingerprints stop being findable after their window; the
                // fingerprint itself stays
                let window = mailrs_mailbox_kevy::ledger::SEGMENT_DAYS * 86_400;
                loop {
                    match st.mailbox.prune_template_segments(now - window, BATCH) {
                        Ok(n) => {
                            total += n;
                            if n < BATCH {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "ledger: pruning fingerprint rows failed");
                            break;
                        }
                    }
                }
                loop {
                    match st.mailbox.prune_expired_links(now, BATCH) {
                        Ok(n) => {
                            total += n;
                            if n < BATCH {
                                return total;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "ledger: pruning link rows failed");
                            return total;
                        }
                    }
                }
            })
            .await
            .unwrap_or(0);
            wait = match pruned {
                0 => (wait * 2).min(MAX),
                n => {
                    tracing::info!(pruned = n, "ledger: link rows past their retention removed");
                    BASE
                }
            };
        }
    });
}

/// Records an inbound message in the ledger, with the sender relationship
/// it adds to. True when this call recorded it. A failure is logged and delivery goes on: the ledger is what
/// later re-judging reads, not what delivering the mail needs.
pub(crate) fn record_delivery(
    state: &FastcoreState,
    user: &str,
    message_id: &str,
    raw: &[u8],
) -> bool {
    let facts = facts_of(user, message_id, raw, state.authserv_id.as_deref());
    let indexed = indexed_link_hosts(&facts);
    match state
        .mailbox
        .record_ledger_indexed(&facts, &indexed, Some(links_until(&facts)))
    {
        Ok(false) => return false,
        Ok(true) => {}
        Err(e) => {
            tracing::warn!(%user, error = %e, "ledger: could not record the message");
            return false;
        }
    }
    crate::report::fingerprint::record(state, user, message_id, raw, facts.arrived);
    let name = mailrs_inbound::from_header(raw);
    if let Err(e) =
        state
            .mailbox
            .record_inbound_relationship(user, &facts.from_addr, &name, facts.arrived)
    {
        tracing::warn!(%user, error = %e, "ledger: could not count the relationship");
    }
    let stored = state.mailbox.fraud_verdict(message_id).ok().flatten();
    let verdict = delivered_verdict(stored.as_deref(), facts.arrived);
    if let Err(e) = state.mailbox.record_verdict(user, message_id, &verdict) {
        tracing::warn!(%user, error = %e, "ledger: could not record the verdict");
    }
    true
}
