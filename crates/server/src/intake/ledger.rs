//! What delivery records in this core's fact ledger about each message,
//! read off the raw message as the kevy core reads it
//! (`mailrs_intake::ledger`), and the fill for mail delivered before the
//! ledger was kept.

use std::sync::Arc;

use mailrs_intake::ledger::{delivered_verdict, facts_of, indexed_link_hosts, links_until};
use mailrs_mailbox::PgMailboxStore;

use super::PgCore;

/// Records `addr`'s copy of an inbound message in the ledger, with the
/// verdict delivery gave it. False when it was there already.
pub(crate) async fn record(
    mailbox: &PgMailboxStore,
    authserv: Option<&str>,
    addr: &str,
    message_id: &str,
    body: &[u8],
    fraud_verdict: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let facts = facts_of(addr, message_id, body, authserv);
    let indexed = indexed_link_hosts(&facts);
    if !mailbox
        .record_ledger_indexed(&facts, &indexed, Some(links_until(&facts)))
        .await?
    {
        return Ok(false);
    }
    let verdict = delivered_verdict(fraud_verdict, facts.arrived);
    mailbox.record_verdict(addr, message_id, &verdict).await?;
    Ok(true)
}

impl PgCore {
    /// [`record`] at delivery. Logged on failure: the ledger is what later
    /// re-judging reads, not what delivering the mail needs.
    pub(super) fn record_ledger(
        &self,
        addr: &str,
        message_id: &str,
        body: &[u8],
        fraud_verdict: Option<&str>,
    ) {
        let recorded = self.dir.rt.block_on(record(
            &self.mailbox,
            self.dir.authserv_id.as_deref(),
            addr,
            message_id,
            body,
            fraud_verdict,
        ));
        if let Err(e) = recorded {
            tracing::warn!(%addr, error = %e, "ledger: could not record the message");
        }
    }
}

const BATCH: i64 = 500;

/// Record, once at startup, the received mail that has no ledger record:
/// mail delivered before the ledger was kept. Every message is looked at
/// once per startup, a page at a time by row id; the user's own mail and
/// mail without a Received line are not delivered mail and stay
/// unrecorded, which is why the pass cannot shrink to nothing.
pub(crate) fn spawn_fill(
    mailbox: Arc<PgMailboxStore>,
    maildir_root: String,
    authserv: Option<String>,
) {
    tokio::spawn(async move {
        let (mut after, mut recorded) = (0, 0u64);
        loop {
            let page = match mailbox.unledgered(after, BATCH).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(error = %e, recorded, "ledger fill failed");
                    return;
                }
            };
            let Some(last) = page.last() else {
                break;
            };
            after = last.0;
            for (_, user, mid, file) in &page {
                let Some(raw) =
                    crate::message_util::read_message_raw(&maildir_root, user, file).await
                else {
                    continue;
                };
                if mailrs_intake::ledger::is_own(user, &raw)
                    || !mailrs_intake::ledger::has_received_line(&raw)
                {
                    continue;
                }
                let verdict = match mailbox.fraud_verdict(mid).await {
                    Ok(v) => v.map(|v| v.to_string()),
                    Err(e) => {
                        tracing::warn!(error = %e, %mid, "ledger fill: the verdict was not read");
                        None
                    }
                };
                match record(
                    &mailbox,
                    authserv.as_deref(),
                    user,
                    mid,
                    &raw,
                    verdict.as_deref(),
                )
                .await
                {
                    Ok(true) => recorded += 1,
                    Ok(false) => {}
                    Err(e) => tracing::warn!(error = %e, %mid, "ledger fill: not recorded"),
                }
            }
        }
        if recorded > 0 {
            tracing::info!(
                recorded,
                "ledger filled in for mail delivered before it was kept"
            );
        }
    });
}
