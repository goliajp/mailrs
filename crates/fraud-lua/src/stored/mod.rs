//! Stored mail through the fraud rules: what is read off a message that
//! is already on disk, the history it is judged against, and the verdict
//! recorded for it.
//!
//! Shared by the core's re-scan of historical mail and by imports, which
//! arrive as stored mail rather than over SMTP. Everything here reads; the
//! live receive path owns every write to the history, so judging the same
//! message twice cannot change what it is judged against.

pub mod qr_domain;
mod reading;
pub mod relation;
pub mod stamp;
pub use reading::{Extracted, Reading};

/// What the rules make of one stored message.
pub struct Judged {
    /// Every rule that fired.
    pub findings: mailrs_fraud::Findings,
    /// The rules would hold it.
    pub holds: bool,
    /// The verdict to store beside it, under the rule set that judged it.
    pub verdict: mailrs_inbound::FraudVerdict,
}

/// Judge `raw`, read as stored mail, against the rules in force.
///
/// `our_stamp` is this deployment's `authserv-id`; `hist` the network
/// store the receive path records sender history in, read and never
/// written. Without it the history reads as empty, which [`domain_seen`]
/// and [`reply_rotation`] explain. `extracted` is what the attachment
/// reader stored for it.
pub fn judge(
    raw: &[u8],
    our_stamp: Option<&str>,
    extracted: Extracted<'_>,
    policy: &mailrs_fraud::Policy,
    hist: &mut Option<kevy_client::Connection>,
) -> Result<Judged, String> {
    judge_for(raw, None, our_stamp, extracted, policy, hist)
}

/// [`judge`], for one reader: `reader` is `(user, when the message is
/// judged relative to its own arrival)`, which adds the reader's history
/// with the sender. Without it those facts are all false.
pub fn judge_for(
    raw: &[u8],
    reader: Option<(&str, relation::Counted)>,
    our_stamp: Option<&str>,
    extracted: Extracted<'_>,
    policy: &mailrs_fraud::Policy,
    hist: &mut Option<kevy_client::Connection>,
) -> Result<Judged, String> {
    judge_with(
        raw,
        reader,
        our_stamp,
        extracted,
        policy,
        hist,
        &mut crate::scan,
    )
}

/// The rules a judgement runs: the receiver's [`crate::scan`], or a rule
/// set a test compiles.
pub type Scanner<'a> =
    dyn FnMut(&mailrs_fraud::Facts<'_>, &mailrs_fraud::Policy) -> Result<crate::Scan, String> + 'a;

/// [`judge_for`] under the rules `scan` runs.
pub fn judge_with(
    raw: &[u8],
    reader: Option<(&str, relation::Counted)>,
    our_stamp: Option<&str>,
    extracted: Extracted<'_>,
    policy: &mailrs_fraud::Policy,
    hist: &mut Option<kevy_client::Connection>,
    scan: &mut Scanner<'_>,
) -> Result<Judged, String> {
    let msg = Reading::of_extracted(raw, our_stamp, extracted);
    let seen = domain_seen(hist.as_mut(), &msg.from);
    let rotation = reply_rotation(hist, &msg.host, &msg.reply_to);
    let rel = match reader {
        Some((user, counted)) => {
            relation::relation(hist.as_mut(), user, &msg.from, &msg.reply_to, counted)
        }
        None => Default::default(),
    };
    // new as of when the message arrived, not as of the re-judging
    let at = msg.arrived.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    let novelty = mailrs_core_sidestate::families::domain_first::novelty(
        hist.as_mut(),
        &msg.host,
        &msg.link_hosts,
        at,
    );
    let new_qr_domain = qr_domain::new_domain(hist.as_mut(), &msg, at);
    let facts = mailrs_fraud::Facts {
        novelty,
        new_qr_domain,
        ..msg.facts(seen, rotation, rel)
    };
    let scan = scan(&facts, policy)?;
    let mut verdict = rescan_verdict(raw, &scan.findings);
    verdict.rules_version = scan.version;
    Ok(Judged {
        holds: mailrs_inbound::holds(&scan.findings),
        findings: scan.findings,
        verdict,
    })
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
pub fn header_value(raw: &[u8], name_lower: &[u8]) -> Option<String> {
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
pub fn rescan_verdict(
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
pub fn domain_seen(conn: Option<&mut kevy_client::Connection>, from: &str) -> u64 {
    let Some(conn) = conn else { return 0 };
    let Some(at) = from.rfind('@') else { return 0 };
    let host = from[at + 1..].trim_end_matches('>').trim();
    mailrs_core_sidestate::families::domain_history::seen(conn, host)
}

/// How many registrable domains send to this message's off-domain
/// reply address.
///
/// **Reads without recording.** The live path owns the writing; a
/// sweep that also wrote would make the count depend on how many
/// times the sweep had run over the same message, and the number
/// that convicts would stop meaning what it says.
///
/// Zero without a store, which reads as *not rotating* — a missing
/// store delivers here rather than holds, the opposite direction to
/// [`domain_seen`] and for the same reason in both cases: be wrong
/// towards the answer that does not hide mail.
pub fn reply_rotation(
    conn: &mut Option<kevy_client::Connection>,
    from_host: &str,
    reply_addr: &str,
) -> u32 {
    let Some(conn) = conn.as_mut() else { return 0 };
    mailrs_core_sidestate::families::reply_rotation::domains(conn, from_host, reply_addr)
}
