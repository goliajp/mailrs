//! Feedback-loop (ARF, RFC 5965) complaint handling.
//!
//! ESPs that run a feedback loop mail us an ARF report every time one
//! of their users hits "this is spam" on a message we sent. Ignoring
//! those reports is the classic way to lose sending reputation: the
//! complaint rate keeps climbing because we keep mailing the person who
//! complained.
//!
//! This ran nowhere in production before. The receiver has an
//! equivalent block in `smtp_session/events/data/mod.rs`, but it sits
//! after the `spool_sink` early return, so in the shipped configuration
//! it is unreachable. Handling belongs here, where messages are
//! actually processed.

use mailrs_core_sidestate::families::suppression;

/// Local-parts that receive feedback-loop reports. Registering an FBL
/// with an ESP requires one of these, so anything else is not a report
/// address and is skipped before the (comparatively expensive) parse.
const FBL_MAILBOXES: [&str; 2] = ["abuse", "postmaster"];

/// Feedback types that mean "stop mailing this person".
///
/// `not-spam` is deliberately excluded — it is the *opposite* signal,
/// sent when a user rescues a message from their spam folder.
/// Suppressing on it would invert the intent. `virus` and `other`
/// describe the message, not the recipient's wishes, so they are left
/// alone too.
const SUPPRESSING_TYPES: [&str; 2] = ["abuse", "fraud"];

/// Parse a message delivered to an FBL mailbox and suppress the
/// complainant.
///
/// Called from the spool drain for every resolved recipient; returns
/// immediately for ordinary mail. Delivery is never affected — the
/// report still lands in the mailbox, and every failure here is logged
/// and swallowed.
pub fn maybe_record_complaint(maildir_root: &str, rcpt: &str, body: &[u8]) {
    if !is_fbl_mailbox(rcpt) {
        return;
    }
    let Some(report) = mailrs_arf::parse(body) else {
        return;
    };
    if !SUPPRESSING_TYPES.contains(&report.feedback_type.as_str()) {
        tracing::info!(
            event = "fbl_report_ignored",
            feedback_type = %report.feedback_type,
            "ARF report is not a complaint; nothing suppressed"
        );
        return;
    }

    // Deliberately NOT `report.complainant()`. That helper falls back to
    // `original_mail_from` when `Original-Rcpt-To` is absent — and in a
    // report about *our* message, `original_mail_from` is one of our own
    // sending addresses. Following that fallback would suppress
    // ourselves and silently stop outbound mail.
    let Some(complainant) = report.original_rcpt_to.as_deref() else {
        tracing::warn!(
            event = "fbl_report_no_rcpt",
            feedback_type = %report.feedback_type,
            "ARF report has no Original-Rcpt-To; cannot identify the complainant"
        );
        return;
    };

    let complainant = suppression::normalize(complainant);
    if complainant.is_empty() {
        return;
    }
    // Second guard on the same hazard: never suppress an address we
    // host. A malformed or hostile report must not be able to take one
    // of our own mailboxes off the air.
    if crate::spool_drain::has_maildir(maildir_root, &complainant) {
        tracing::warn!(
            event = "fbl_report_self_target",
            address = %complainant,
            "ARF report named a local mailbox; refusing to suppress"
        );
        return;
    }

    let Some(url) = crate::live_sync::network_kevy_url() else {
        tracing::warn!("no network kevy — FBL complaint not recorded");
        return;
    };
    let Ok(mut conn) = kevy_client::Connection::open(&url) else {
        tracing::warn!(address = %complainant, "FBL: no kevy connection");
        return;
    };
    let reason = format!("FBL complaint: {}", report.feedback_type);
    match suppression::add(
        &mut conn,
        &complainant,
        suppression::Source::Complaint,
        &reason,
        now_secs(),
    ) {
        Ok(()) => tracing::info!(
            event = "fbl_complaint_recorded",
            address = %complainant,
            feedback_type = %report.feedback_type,
            reported_domain = %report.reported_domain.as_deref().unwrap_or("-"),
            "suppressed complainant permanently"
        ),
        Err(e) => tracing::warn!(error = %e, address = %complainant, "FBL: suppression failed"),
    }
}

/// Whether `addr`'s local-part is one of the FBL report mailboxes.
fn is_fbl_mailbox(addr: &str) -> bool {
    let Some((local, _)) = addr.split_once('@') else {
        return false;
    };
    let local = local.to_ascii_lowercase();
    FBL_MAILBOXES.contains(&local.as_str())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_fbl_mailboxes() {
        assert!(is_fbl_mailbox("abuse@golia.jp"));
        assert!(is_fbl_mailbox("postmaster@golia.ai"));
        assert!(is_fbl_mailbox("ABUSE@golia.jp"));
    }

    #[test]
    fn ordinary_recipients_are_not_fbl_mailboxes() {
        assert!(!is_fbl_mailbox("lihao@golia.jp"));
        assert!(!is_fbl_mailbox("dmarc@golia.jp"));
        // substring, not the local-part
        assert!(!is_fbl_mailbox("abuse-team@golia.jp"));
        assert!(!is_fbl_mailbox("no-at-sign"));
    }

    #[test]
    fn only_complaint_types_suppress() {
        assert!(SUPPRESSING_TYPES.contains(&"abuse"));
        assert!(SUPPRESSING_TYPES.contains(&"fraud"));
        // The inverse signal must never suppress.
        assert!(!SUPPRESSING_TYPES.contains(&"not-spam"));
        assert!(!SUPPRESSING_TYPES.contains(&"virus"));
        assert!(!SUPPRESSING_TYPES.contains(&"other"));
    }

    /// Guards the exact hazard described above `original_rcpt_to`:
    /// `complainant()` would hand back our own sending address here.
    #[test]
    fn complainant_helper_would_return_our_own_address_without_rcpt_to() {
        let report = mailrs_arf::Report {
            feedback_type: "abuse".into(),
            user_agent: None,
            version: None,
            original_mail_from: Some("noreply@golia.jp".into()),
            original_rcpt_to: None,
            arrival_date: None,
            source_ip: None,
            reported_domain: None,
            reported_uri: None,
            authentication_results: None,
            incidents: None,
        };

        assert_eq!(
            report.complainant(),
            Some("noreply@golia.jp"),
            "documents why this module reads original_rcpt_to directly"
        );
        assert_eq!(report.original_rcpt_to, None, "so nothing gets suppressed");
    }
}
