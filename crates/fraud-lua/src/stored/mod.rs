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
    // new as of when the message arrived, not as of the re-judging
    let at = msg.arrived.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    let rel = match reader {
        Some((user, counted)) => mailrs_fraud::relation::Relation {
            account_changed: relation::account_changed(
                hist.as_mut(),
                user,
                &msg.from,
                msg.account_keys(),
                at,
            ),
            ..relation::relation(hist.as_mut(), user, &msg.from, &msg.reply_to, counted)
        },
        None => Default::default(),
    };
    let novelty = mailrs_core_sidestate::families::domain_first::novelty(
        hist.as_mut(),
        &msg.host,
        &msg.link_hosts,
        at,
    );
    let new_qr_domain = qr_domain::new_domain(hist.as_mut(), &msg, at);
    let mut facts = mailrs_fraud::Facts {
        novelty,
        new_qr_domain,
        ..msg.facts(seen, rotation, rel)
    };
    let site = site(
        hist.as_mut(),
        &facts,
        policy,
        reader.map_or("", |(user, _)| user),
        raw,
    );
    facts.site_elsewhere = site.elsewhere;
    facts.landing_known_domain = site.landing_known_domain;
    facts.copies_someone_known = site.copies_someone_known;
    let scan = scan(&facts, policy)?;
    let mut verdict = rescan_verdict(raw, &scan.findings);
    verdict.rules_version = scan.version;
    Ok(Judged {
        holds: mailrs_inbound::holds(&scan.findings),
        findings: scan.findings,
        verdict,
    })
}

/// The burner-domain facts of one message
/// ([`mailrs_fraud::burner_domain`]): whether the sender's web root lands
/// on another domain, and the two things that excuse it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Site {
    /// The web root lands on another registrable domain, not a mailbox
    /// provider's.
    pub elsewhere: bool,
    /// The reader knows the company: written to the domain it lands on,
    /// or released it from this rule by marking its mail not junk.
    pub landing_known_domain: bool,
    /// The `To` or `Cc` names someone the reader knows
    /// ([`relation::copies_someone_known`]).
    pub copies_someone_known: bool,
}

/// [`Site`] as the receive path recorded the landing when it visited.
/// Read only for a message [`mailrs_fraud::burner_domain::worth_visiting`],
/// the same test the receive path visits on; never visits itself, so a
/// host it has no record of reads as not elsewhere.
pub fn site(
    conn: Option<&mut kevy_client::Connection>,
    facts: &mailrs_fraud::Facts<'_>,
    policy: &mailrs_fraud::Policy,
    reader: &str,
    raw: &[u8],
) -> Site {
    if !mailrs_fraud::burner_domain::worth_visiting(
        facts.hosted_mailbox,
        facts.is_bulk,
        &facts.relation,
    ) {
        return Site::default();
    }
    let Some(conn) = conn else {
        return Site::default();
    };
    let Some(landing) = mailrs_core_sidestate::families::site_landing::get(conn, facts.domain)
        .ok()
        .flatten()
    else {
        return Site::default();
    };
    site_from(conn, facts.domain, &landing, policy, reader, raw)
}

/// [`Site`] for a landing in hand: the receive path's, just visited or
/// read, and [`site`]'s. The two excuses are read only when it lands
/// elsewhere, the one case they change.
pub fn site_from(
    conn: &mut kevy_client::Connection,
    from_host: &str,
    landing: &mailrs_fraud::burner_domain::Landing,
    policy: &mailrs_fraud::Policy,
    reader: &str,
    raw: &[u8],
) -> Site {
    if !landing.elsewhere(from_host, &policy.mailbox_providers) {
        return Site::default();
    }
    Site {
        elsewhere: true,
        landing_known_domain: known_domain(conn, reader, from_host, landing),
        copies_someone_known: relation::copies_someone_known(
            conn,
            reader,
            raw,
            &policy.our_domains,
        ),
    }
}

/// Whether the reader knows the company behind a sender whose site lands
/// elsewhere: they have written to someone at the domain it lands on, or
/// they marked mail from it not junk, which released the sender's domain
/// and the one it lands on for them
/// ([`mailrs_core_sidestate::families::burner_released`]).
fn known_domain(
    conn: &mut kevy_client::Connection,
    reader: &str,
    from_host: &str,
    landing: &mailrs_fraud::burner_domain::Landing,
) -> bool {
    use mailrs_core_sidestate::families::burner_released;
    let landed = landing.registrable();
    let from = mailrs_fraud::brand::registrable(from_host);
    relation::wrote_to_domain(conn, reader, &landed)
        || burner_released::knows(conn, reader, &landed).unwrap_or(false)
        || burner_released::knows(conn, reader, &from).unwrap_or(false)
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

#[cfg(test)]
mod site_tests {
    use super::*;
    use mailrs_core_sidestate::families::{burner_released, site_landing};
    use mailrs_fraud::burner_domain::{DEFAULT_MAILBOX_PROVIDERS, Landing};

    const ME: &str = "lihao@golia.jp";
    const RAW: &[u8] = b"From: x@y.com\r\nTo: lihao@golia.jp\r\n\r\nhi";

    fn policy() -> mailrs_fraud::Policy {
        mailrs_fraud::Policy {
            mailbox_providers: DEFAULT_MAILBOX_PROVIDERS.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn known(c: &mut kevy_client::Connection, host: &str) -> bool {
        let landing = site_landing::get(c, host).unwrap().unwrap();
        let site = site_from(c, host, &landing, &policy(), ME, RAW);
        assert!(site.elsewhere, "{host} lands elsewhere");
        site.landing_known_domain
    }

    #[test]
    fn a_release_covers_the_senders_other_domains_on_the_same_site_until_marked_junk() {
        let mut c = kevy_client::Connection::connect("mem://site-tests-release").unwrap();
        for host in ["dealclosersvoice.com", "247dealclosers.com"] {
            site_landing::put(&mut c, host, &Landing::At("dealclosers.co".into())).unwrap();
        }
        assert!(!known(&mut c, "dealclosersvoice.com"));
        assert!(!known(&mut c, "247dealclosers.com"));

        burner_released::released(&mut c, ME, "dealclosersvoice.com").unwrap();
        // the same domain (any address: the fact is the domain's) and a
        // sister domain landing on the same site
        assert!(known(&mut c, "dealclosersvoice.com"));
        assert!(known(&mut c, "247dealclosers.com"));

        burner_released::junked(&mut c, ME, "dealclosersvoice.com").unwrap();
        assert!(!known(&mut c, "dealclosersvoice.com"));
        assert!(!known(&mut c, "247dealclosers.com"));
    }
}
