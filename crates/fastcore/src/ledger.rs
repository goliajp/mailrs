//! What delivery records in the fact ledger about each message.

use mailrs_mailbox_kevy::ledger::LedgerFacts;

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
