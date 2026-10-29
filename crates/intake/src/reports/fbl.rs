//! Feedback-loop (ARF, RFC 5965) complaint handling.
//!
//! ESPs that run a feedback loop mail us an ARF report every time one
//! of their users hits "this is spam" on a message we sent. Ignoring
//! those reports is the classic way to lose sending reputation: the
//! complaint rate keeps climbing because we keep mailing the person who
//! complained.
//!
//! Anyone can write such a report, so one suppresses only when the
//! message it carries is one we sent to the person complaining, and a
//! reporter past its daily limit suppresses nothing more and raises an
//! alert (`fbl_evidence`). Every report is audited (`fbl.report`).

use mailrs_core_sidestate::families::suppression;

use super::fbl_evidence as evidence;

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
    if crate::maildir::has_maildir(maildir_root, &complainant) {
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
    let Ok(mut conn) = kevy_client::Connection::connect(&url) else {
        tracing::warn!(address = %complainant, "FBL: no kevy connection");
        return;
    };
    let reporter = evidence::reporter_domain(body);
    let original = evidence::embedded_original(body);
    let matched = match &original {
        Some(o) => evidence::we_sent_it(&mut conn, o, &complainant).unwrap_or_else(|e| {
            tracing::warn!(error = %e, address = %complainant, "FBL: send record unreadable");
            false
        }),
        None => false,
    };
    let message_id = original.as_ref().map_or("-", |o| o.message_id.as_str());
    let detail = format!(
        "reporter={reporter} type={} message_id={message_id} matched={matched}",
        report.feedback_type
    );
    crate::live_sync::audit_system("fbl.report", &complainant, &detail);
    // On the report's word alone nothing is suppressed: the message it
    // complains about has to be one we sent to the person complaining.
    if !matched {
        tracing::warn!(
            event = "fbl_report_unmatched",
            address = %complainant,
            %reporter,
            %message_id,
            "ARF report does not match a message we sent; nothing suppressed"
        );
        return;
    }

    let day = chrono::Utc::now().format("%Y%m%d").to_string();
    match evidence::within_limit(&mut conn, &reporter, &day) {
        Ok(true) => {}
        Ok(false) => {
            crate::live_sync::audit_system("fbl.rate_limited", &complainant, &detail);
            tracing::warn!(event = "fbl_rate_limited", %reporter, address = %complainant,
                "FBL reporter past its daily limit; not suppressed");
            if evidence::first_alert(&mut conn, &reporter, &day).unwrap_or(false) {
                evidence::send_alert(
                    &format!("FBL complaints from {reporter} past the daily limit"),
                    &format!(
                        "{reporter} has sent more than {} matched feedback-loop complaints today (UTC {day}).\n\
                         Complaints past the limit are logged and audited (action fbl.rate_limited) but do not suppress anyone.\n\
                         Latest: {detail} address={complainant}",
                        evidence::DAILY_LIMIT
                    ),
                );
            }
            return;
        }
        Err(e) => {
            tracing::warn!(error = %e, %reporter, "FBL: rate counter unavailable; not suppressed");
            return;
        }
    }

    let reason = format!(
        "FBL complaint: {} from {reporter} about {message_id}",
        report.feedback_type
    );
    match suppression::add(
        &mut conn,
        &complainant,
        suppression::Source::Complaint,
        &reason,
        now_secs(),
    ) {
        Ok(()) => {
            crate::live_sync::audit_system("fbl.suppressed", &complainant, &detail);
            tracing::info!(
                event = "fbl_complaint_recorded",
                address = %complainant,
                feedback_type = %report.feedback_type,
                %reporter,
                "suppressed complainant"
            );
        }
        Err(e) => tracing::warn!(error = %e, address = %complainant, "FBL: suppression failed"),
    }
}

/// Whether `addr`'s local-part is one of the FBL report mailboxes.
/// Test-visible alias for [`is_fbl_mailbox`], so the spool drain can
/// assert which address it must be asked about.
#[cfg(test)]
pub(crate) fn is_fbl_mailbox_for_test(addr: &str) -> bool {
    is_fbl_mailbox(addr)
}

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
