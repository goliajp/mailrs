//! Authenticated submission to recipients outside the domains we host.
//!
//! A mail client submitting on 587 / 465 hands the receiver one message
//! for every recipient. The receiver spools it like inbound mail; the
//! recipients we host are delivered by the drain, and the rest are put on
//! the outbound queue here for `mailrs-fastcore-sender` to sign and send.

use std::sync::LazyLock;

static LOCAL_DOMAINS: LazyLock<Vec<String>> = LazyLock::new(|| {
    let domains: Vec<String> = std::env::var("MAILRS_LOCAL_DOMAINS")
        .unwrap_or_default()
        .split(',')
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    if domains.is_empty() {
        tracing::warn!(
            "MAILRS_LOCAL_DOMAINS is empty: every recipient of an authenticated \
             submission is treated as remote and relayed outward"
        );
    }
    domains
});

pub(super) fn local_domains() -> &'static [String] {
    &LOCAL_DOMAINS
}

/// Whether `rcpt` (already lowercased) leaves through the outbound queue
/// instead of being delivered here. Only an authenticated session can
/// relay: the receiver answers 550 to an unauthenticated one before it
/// spools anything addressed outside our domains.
pub(super) fn relays_outward(is_authenticated: bool, rcpt: &str, local_domains: &[String]) -> bool {
    let Some((_, domain)) = rcpt.rsplit_once('@') else {
        return false;
    };
    is_authenticated && !local_domains.iter().any(|d| d == domain)
}

/// One outbound job for `rcpt`. The envelope sender is the submitting
/// user's own address, so there is no SRS rewrite as there is for a
/// forward.
pub(super) fn enqueue_submission(sender: &str, rcpt: &str, body: &[u8]) -> std::io::Result<()> {
    use base64::Engine as _;
    let url = std::env::var("MAILRS_KEVY_URL")
        .map_err(|_| std::io::Error::other("MAILRS_KEVY_URL unset"))?;
    let mut conn = kevy_client::Connection::connect(&url)?;
    let b64_body = base64::engine::general_purpose::STANDARD.encode(body);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let sender = sender.trim().trim_matches(['<', '>']);
    let id = mailrs_core_sidestate::families::outbound::write_fresh_pending(
        &mut conn,
        &mailrs_core_sidestate::families::outbound::FreshPending {
            sender,
            recipient: rcpt,
            message_data_base64: &b64_body,
            scheduled_at: None,
            original_sender: None,
            // composed in a mail client, not in the web UI's Send list
            send_id: None,
        },
        now,
    )?;
    tracing::info!(from = %sender, to = %rcpt, job = id, "submission queued for relay");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::relays_outward;

    fn ours() -> Vec<String> {
        vec!["golia.jp".into(), "golia.ai".into()]
    }

    #[test]
    fn an_authenticated_submission_to_another_domain_is_relayed() {
        assert!(relays_outward(true, "someone@example.com", &ours()));
    }

    #[test]
    fn a_recipient_we_host_is_delivered_here_even_when_authenticated() {
        assert!(!relays_outward(true, "lihao@golia.jp", &ours()));
        assert!(!relays_outward(true, "sales@golia.ai", &ours()));
    }

    /// The receiver refuses these, but a spool file is a file: one that
    /// says unauthenticated never becomes outbound mail.
    #[test]
    fn an_unauthenticated_envelope_is_never_relayed() {
        assert!(!relays_outward(false, "someone@example.com", &ours()));
    }

    /// A subdomain of ours is not ours unless it is listed.
    #[test]
    fn only_the_exact_domain_counts_as_ours() {
        assert!(relays_outward(true, "a@mail.golia.jp", &ours()));
        assert!(relays_outward(true, "a@notgolia.jp", &ours()));
    }

    #[test]
    fn an_address_without_a_domain_is_not_relayed() {
        assert!(!relays_outward(true, "postmaster", &ours()));
    }
}
