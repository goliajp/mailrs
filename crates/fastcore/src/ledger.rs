//! What delivery records in the fact ledger about each message.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use mailrs_mailbox_kevy::ledger::LedgerFacts;

use crate::FastcoreState;

/// How long a link row is kept, in days: `MAILRS_LEDGER_LINK_DAYS`, 180 by
/// default. Re-judging older mail when a host's reputation changes is not
/// worth an index that grows for ever.
fn link_days() -> i64 {
    std::env::var("MAILRS_LEDGER_LINK_DAYS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(180)
}

/// The never-list with this server's domains: a host on it never gets an
/// automatic reputation change, so mail is never found by it.
static NEVER: LazyLock<mailrs_reputation::NeverList> = LazyLock::new(|| {
    let own: Vec<String> = std::env::var("MAILRS_LOCAL_DOMAINS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    mailrs_reputation::NeverList::builtin(&own)
});

/// The link hosts worth a row: not the sender's own domain (its domain index
/// already finds that mail) and not on the never-list.
pub(crate) fn indexed_link_hosts(f: &LedgerFacts) -> Vec<String> {
    let sender = mailrs_reputation::registrable(&f.from_domain);
    f.link_hosts
        .iter()
        .filter(|h| mailrs_reputation::registrable(h) != sender && !NEVER.contains(h))
        .cloned()
        .collect()
}

/// When `f`'s link rows stop being worth keeping.
pub(crate) fn links_until(f: &LedgerFacts) -> i64 {
    let arrived = match f.arrived {
        0 => chrono::Utc::now().timestamp(),
        at => at,
    };
    arrived + link_days() * 86_400
}

/// Remove link rows past their retention, hourly; backs off to six hours
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

/// The ledger's facts for one stored message, read off its raw bytes.
pub(crate) fn facts_of(user: &str, message_id: &str, raw: &[u8]) -> LedgerFacts {
    let parsed = mailrs_mime::parse(raw);
    let from_addr = mailrs_inbound::identity::from_address(raw).to_ascii_lowercase();
    let from_domain = from_addr
        .rsplit_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_default();
    let attachment_sha256 = mailrs_inbound::scan_input::attachments(&parsed)
        .iter()
        .map(|a| {
            use sha2::Digest as _;
            sha2::Sha256::digest(a.bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        })
        .collect();
    let (spf, dkim, dmarc) = mailrs_inbound::identity::auth_results_tokens(raw);
    LedgerFacts {
        message_id: message_id.to_string(),
        user: user.to_string(),
        from_addr,
        from_domain,
        reply_to: mailrs_inbound::identity::reply_to_address(raw),
        link_hosts: mailrs_inbound::scan_input::link_hosts(&parsed),
        attachment_sha256,
        auth: format!("{spf}/{dkim}/{dmarc}"),
        arrived: mailrs_inbound::dates::received_epoch(raw).unwrap_or(0),
    }
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
    let facts = facts_of(user, message_id, raw);
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
    let name = mailrs_inbound::from_header(raw);
    if let Err(e) =
        state
            .mailbox
            .record_inbound_relationship(user, &facts.from_addr, &name, facts.arrived)
    {
        tracing::warn!(%user, error = %e, "ledger: could not count the relationship");
    }
    true
}

/// Whether `user` wrote this message, the way delivery decides it.
pub(crate) fn is_own(user: &str, raw: &[u8]) -> bool {
    let (_, _, _, _, _, from, _) = mailrs_intake::headers::extract_headers(raw);
    mailrs_rfc5322::list_contains(&from, user)
}

#[cfg(test)]
mod link_tests {
    use super::*;

    fn facts(domain: &str, hosts: &[&str], arrived: i64) -> LedgerFacts {
        LedgerFacts {
            message_id: "m@x".into(),
            user: "u@x".into(),
            from_addr: format!("news@{domain}"),
            from_domain: domain.into(),
            reply_to: String::new(),
            link_hosts: hosts.iter().map(|h| h.to_string()).collect(),
            attachment_sha256: Vec::new(),
            auth: String::new(),
            arrived,
        }
    }

    /// The sender's own domain is found by its domain index, and a
    /// never-listed host is never re-judged: neither gets a row.
    #[test]
    fn only_hosts_worth_finding_mail_by_get_a_row() {
        let f = facts(
            "mail.shop.example",
            &[
                "shop.example",
                "img.shop.example",
                "docs.google.com",
                "pay-here.example",
            ],
            1,
        );
        assert_eq!(indexed_link_hosts(&f), vec!["pay-here.example"]);
    }

    #[test]
    fn link_rows_are_kept_for_the_retention_from_arrival() {
        let f = facts("shop.example", &[], 1_000_000);
        assert_eq!(links_until(&f), 1_000_000 + link_days() * 86_400);
    }
}
