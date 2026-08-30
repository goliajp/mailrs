//! Reading one delivered message back off the disk.
//!
//! Split from the sweep by theme: the sweep decides, this fetches.
//! Both halves have gone wrong on their own — the sweep judged
//! against a threshold one lower than the corpus was measured at,
//! and this side missed a folded `Message-ID` for a day — and they
//! are easier to keep honest apart.
//!
//! Everything here answers the same question the receive path
//! answers about a message arriving now, through the same readers in
//! `mailrs_inbound`. A second reader of one header is how the folded
//! case came to work on one path and not the other.

use super::super::prelude::*;

/// The raw bytes of a thread's newest message, from this user's copy.
pub(super) fn newest_raw(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
) -> Option<(String, Vec<u8>)> {
    let mut newest: Option<(i64, String, String)> = None;
    for mid in state
        .mailbox
        .user_thread_message_ids(user, tid)
        .unwrap_or_default()
    {
        let Ok(Some(bytes)) = state.mailbox.user_message_view(user, &mid) else {
            continue;
        };
        let Ok(wire) =
            serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes)
        else {
            continue;
        };
        if wire.blob_ref.is_empty() {
            continue;
        }
        let better = match &newest {
            None => true,
            Some((d, _, _)) => wire.date >= *d,
        };
        if better {
            newest = Some((wire.date, wire.blob_ref, mid.clone()));
        }
    }
    // The row's own id, not one re-derived from the file. Three held
    // conversations on production carry a synthetic
    // `…@mailrs.local` id while the file underneath has a real
    // `Message-ID`, so a verdict keyed off the file was stored where
    // no reader looks: the screen asks with the id the message row
    // carries, and got null for a conversation it was showing as
    // held. Two ids for one message, the verdict under the one nobody
    // reads.
    let (_, blob_ref, message_id) = newest?;
    let raw = read_maildir_file(user, &blob_ref)?;
    Some((message_id, raw))
}

/// One header's value, by its lowercase name including the colon,
/// with its continuation lines joined.
///
/// Stops at the blank line, so a quoted header in the body is not a
/// header.
///
/// **Folded headers are joined**, and the comment here used to say
/// they were not — "the three that matter put theirs on the first
/// line". Production disproved it within a minute of the first sweep:
/// Exchange writes
///
/// ```text
/// Message-ID:
///  <SA5PR03MB8426…@…outlook.com>
/// ```
///
/// with nothing after the colon, so the id read as empty and the
/// held conversation could not explain itself. `X-Mailer` reads
/// through the same function, so a folded one would have been missed
/// by the scan itself — a fraud check that silently does not fire.
///
/// RFC 5322 §2.2.3: a line beginning with space or tab continues the
/// previous field. Joined with a single space, which is what
/// unfolding means for a structured value.
pub(super) fn header_value(raw: &[u8], name_lower: &[u8]) -> Option<String> {
    let head = &raw[..raw.len().min(16 * 1024)];
    let text = String::from_utf8_lossy(head);
    let name = String::from_utf8_lossy(name_lower);
    let mut value: Option<String> = None;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        if let Some(v) = &mut value {
            // Still inside the field while the line is folded.
            match line.starts_with([' ', '\t']) {
                true => {
                    if !v.is_empty() {
                        v.push(' ');
                    }
                    v.push_str(line.trim());
                    continue;
                }
                false => break,
            }
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix(name.as_ref()) {
            value = Some(line[line.len() - rest.len()..].trim().to_string());
        }
    }
    value.filter(|v| !v.is_empty())
}

/// A verdict for mail that arrived before the checks existed.
///
/// Honest about what it does and does not know. The identity and
/// provenance layers are re-derived from the message, which is where
/// they came from in the first place. The transport layer is read from
/// the `Authentication-Results` header the receiver wrote at the time —
/// that is the receipt, and reading it is not the same as re-running
/// the check. Where the header is missing the layer reports
/// not-applicable, which is what it is.
pub(super) fn rescan_verdict(
    raw: &[u8],
    findings: &mailrs_fraud::Findings,
) -> mailrs_inbound::FraudVerdict {
    let mut input = mailrs_inbound::unexamined();
    input.fraud = findings.clone();
    if let Some(value) = header_value(raw, b"authentication-results:") {
        for r in mailrs_inbound::auth_header::parse_auth_results(&value) {
            match r.method.as_str() {
                "spf" => input.auth.spf = r.result,
                "dkim" => input.auth.dkim = r.result,
                "dmarc" => input.auth.dmarc = r.result,
                "arc" => input.auth.arc = r.result,
                _ => {}
            }
        }
    }
    mailrs_inbound::assess(&input)
}

/// How many messages this deployment has had from the sender's
/// domain, or zero when there is no history store to ask.
///
/// Zero is the unfamiliar answer, which makes a brand claim
/// suspicious — so a missing store fails towards holding rather than
/// towards delivering. The warning above says when that is happening,
/// because "the check found nothing" and "the check could not look"
/// come back as the same count.
pub(super) fn domain_seen(conn: Option<&mut kevy_client::Connection>, from: &str) -> u64 {
    let Some(conn) = conn else { return 0 };
    let Some(at) = from.rfind('@') else { return 0 };
    let host = from[at + 1..].trim_end_matches('>').trim();
    mailrs_core_sidestate::families::domain_history::seen(conn, host)
}
