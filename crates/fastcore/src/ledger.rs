//! What delivery records in the fact ledger about each message.

use mailrs_mailbox_kevy::ledger::LedgerFacts;

use crate::FastcoreState;

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
    match state.mailbox.record_ledger(&facts) {
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
