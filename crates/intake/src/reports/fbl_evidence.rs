//! What makes an ARF complaint worth acting on.
//!
//! Anyone can mail a well-formed ARF report to abuse@ or postmaster@, and
//! until 2.135.4 one naming any outside address suppressed our mail to it
//! for good. A complaint now counts only when the message it carries is
//! one we sent to the person complaining, and no single reporter can
//! suppress more than [`DAILY_LIMIT`] addresses a day.

use std::time::Duration;

use kevy_client::Connection;
use mailrs_core_sidestate::families::send;

/// Suppressions one reporting domain may cause in a UTC day. Real
/// feedback loops send a handful a day for a server this size; a burst
/// past this is either a mass complaint worth a person's look or a
/// forgery campaign, and in both cases a person decides.
pub const DAILY_LIMIT: i64 = 20;

/// The complained-about message, as the report carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Original {
    /// Bare `From:` address, lowercased.
    pub from: String,
    /// `Message-ID:` without the angle brackets.
    pub message_id: String,
}

/// The original message (`message/rfc822`) or its headers
/// (`text/rfc822-headers`) inside the report, read for the two fields a
/// match needs. `None` when the report carries neither, or they lack a
/// `From:` or `Message-ID:`.
pub fn embedded_original(report: &[u8]) -> Option<Original> {
    let root = mailrs_mime::parse(report);
    let part = root.walk().find(|p| {
        matches!(
            p.content_type.mime_type().to_ascii_lowercase().as_str(),
            "message/rfc822" | "text/rfc822-headers"
        )
    })?;
    let from = mailrs_inbound::identity::from_address(&part.body);
    let message_id = mailrs_rfc5322::Message::new(&part.body)
        .header_str("Message-ID")?
        .trim()
        .trim_matches(['<', '>'])
        .to_string();
    (!from.is_empty() && !message_id.is_empty()).then_some(Original { from, message_id })
}

/// The domain the report itself came from, by its `From:`. Used to
/// count and to name the reporter; it proves nothing about who that is.
pub fn reporter_domain(report: &[u8]) -> String {
    let from = mailrs_inbound::identity::from_address(report);
    match from.rsplit_once('@') {
        Some((_, d)) if !d.is_empty() => d.to_string(),
        _ => "unknown".to_string(),
    }
}

/// Whether `original` is a message our `original.from` account sent to
/// `complainant`, by that account's send record.
///
/// Mail sent over SMTP submission keeps no send record, so a complaint
/// about it does not match and is logged instead of acted on.
pub fn we_sent_it(
    conn: &mut Connection,
    original: &Original,
    complainant: &str,
) -> std::io::Result<bool> {
    let rcpts = conn.hgetall(send::rcpt_key(&original.from, &original.message_id).as_bytes())?;
    Ok(rcpts.iter().step_by(2).any(|r| {
        String::from_utf8_lossy(r)
            .trim()
            .eq_ignore_ascii_case(complainant)
    }))
}

/// Count one suppression against `reporter` for `day` (`YYYYMMDD`);
/// `false` once the reporter is past [`DAILY_LIMIT`].
pub fn within_limit(conn: &mut Connection, reporter: &str, day: &str) -> std::io::Result<bool> {
    let key = format!("mailrs:fbl:count:{reporter}:{day}");
    let n = conn.incr(key.as_bytes())?;
    if n == 1 {
        conn.expire(key.as_bytes(), Duration::from_secs(2 * 86_400))?;
    }
    Ok(n <= DAILY_LIMIT)
}

/// `true` for the first over-limit report of `reporter` on `day`, so one
/// burst raises one alert rather than one per report.
pub fn first_alert(conn: &mut Connection, reporter: &str, day: &str) -> std::io::Result<bool> {
    let key = format!("mailrs:fbl:alerted:{reporter}:{day}");
    let n = conn.incr(key.as_bytes())?;
    if n == 1 {
        conn.expire(key.as_bytes(), Duration::from_secs(2 * 86_400))?;
    }
    Ok(n == 1)
}

/// Tell the operators, through the security alert queue every security
/// mail goes out from, that `reporter` is past its daily limit.
pub fn raise_alert(conn: &mut Connection, reporter: &str, complainant: &str, detail: &str) {
    use mailrs_auth_guard::alerts::{self, Alert, Fact, Kind};
    let alert = Alert {
        kind: Kind::ComplaintLimit,
        subject: reporter.to_string(),
        at: chrono::Utc::now().timestamp().max(0) as u64,
        detail: format!(
            "more than {DAILY_LIMIT} matched complaints today; latest: {detail} address={complainant}"
        ),
        facts: vec![
            Fact::Complaints {
                reporter: reporter.to_string(),
                limit: DAILY_LIMIT as u64,
            },
            Fact::LatestComplaint {
                address: complainant.to_string(),
            },
        ],
    };
    if let Err(e) = alerts::raise(conn, &alert) {
        tracing::warn!(error = %e, %reporter, "FBL alert could not be queued");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(original_part: &str) -> Vec<u8> {
        format!(
            "From: Feedback <fbl@reporter.example>\r\n\
             To: abuse@golia.jp\r\n\
             Content-Type: multipart/report; report-type=feedback-report; boundary=\"b\"\r\n\r\n\
             --b\r\nContent-Type: text/plain\r\n\r\nA complaint.\r\n\
             --b\r\nContent-Type: message/feedback-report\r\n\r\n\
             Feedback-Type: abuse\r\nUser-Agent: test/1\r\nVersion: 1\r\n\
             Original-Rcpt-To: <victim@outside.example>\r\n\r\n\
             --b\r\n{original_part}\
             --b--\r\n"
        )
        .into_bytes()
    }

    #[test]
    fn the_original_message_is_read_from_either_carrier() {
        for ct in ["message/rfc822", "text/rfc822-headers"] {
            let r = report(&format!(
                "Content-Type: {ct}\r\n\r\n\
                 From: Qa <QA@golia.jp>\r\nMessage-ID: <abc123@golia.jp>\r\nSubject: hi\r\n\r\nbody\r\n"
            ));
            assert_eq!(
                embedded_original(&r),
                Some(Original {
                    from: "qa@golia.jp".into(),
                    message_id: "abc123@golia.jp".into()
                }),
                "{ct}"
            );
        }
    }

    #[test]
    fn a_report_without_its_original_has_nothing_to_match() {
        assert_eq!(embedded_original(&report("")), None);
        let no_id = report("Content-Type: message/rfc822\r\n\r\nFrom: qa@golia.jp\r\n\r\nbody\r\n");
        assert_eq!(embedded_original(&no_id), None);
    }

    #[test]
    fn the_reporter_is_named_by_its_from_domain() {
        assert_eq!(reporter_domain(&report("")), "reporter.example");
        assert_eq!(reporter_domain(b"Subject: x\r\n\r\n"), "unknown");
    }
}
