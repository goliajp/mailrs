//! What delivery records in the fact ledger about each message, the same
//! for every store: the facts read off the raw message, the link hosts
//! worth an index row and how long, and the verdicts recorded beside them.

use mailrs_mailbox::ledger::{IndexedLink, LedgerFacts, LedgerVerdict};

use crate::attachment_text::NEVER;

/// How long a link row is kept, in days: `MAILRS_LEDGER_LINK_DAYS`, 180 by
/// default. Re-judging older mail when a host's reputation changes is not
/// worth an index that grows for ever.
pub fn link_days() -> i64 {
    std::env::var("MAILRS_LEDGER_LINK_DAYS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(180)
}

/// The link hosts worth a row: not the sender's own domain (its domain index
/// already finds that mail) and not on the never-list.
pub fn indexed_link_hosts(f: &LedgerFacts) -> Vec<IndexedLink> {
    let sender = mailrs_reputation::registrable(&f.from_domain);
    f.link_hosts
        .iter()
        .filter(|h| mailrs_reputation::registrable(h) != sender && !NEVER.contains(h))
        .map(|h| IndexedLink {
            host: h.clone(),
            reg: mailrs_reputation::registrable(h),
        })
        .collect()
}

/// When `f`'s link rows stop being worth keeping.
pub fn links_until(f: &LedgerFacts) -> i64 {
    let arrived = match f.arrived {
        0 => chrono::Utc::now().timestamp(),
        at => at,
    };
    arrived + link_days() * 86_400
}

/// The ledger's facts for one stored message, read off its raw bytes.
/// `authserv` is this server's name: a results header in that name its
/// receiver did not write is not read.
pub fn facts_of(user: &str, message_id: &str, raw: &[u8], authserv: Option<&str>) -> LedgerFacts {
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
    let (spf, dkim, dmarc) = mailrs_inbound::identity::auth_results_tokens_trusted(raw, authserv);
    LedgerFacts {
        message_id: message_id.to_string(),
        user: user.to_string(),
        from_addr,
        reply_to: mailrs_inbound::identity::reply_to_address(raw),
        link_hosts: mailrs_inbound::scan_input::link_hosts(&parsed),
        attachment_sha256,
        auth: format!("{spf}/{dkim}/{dmarc}"),
        arrived: mailrs_inbound::dates::received_epoch(raw).unwrap_or(0),
        from_reg: mailrs_reputation::registrable(&from_domain),
        from_domain,
    }
}

/// What the receiver made of a message, as a ledger verdict. Its verdict
/// names layers, not rules, so the rules recorded are the layers that
/// failed; a re-judge records rules. No stored verdict is the receiver
/// having found nothing, which is a verdict too.
pub fn delivered_verdict(stored: Option<&str>, arrived: i64) -> LedgerVerdict {
    let stored =
        stored.and_then(|json| serde_json::from_str::<mailrs_inbound::FraudVerdict>(json).ok());
    let Some(v) = stored else {
        return LedgerVerdict {
            judged_at: arrived,
            ..Default::default()
        };
    };
    LedgerVerdict {
        rules: v
            .layers
            .iter()
            .filter(|l| l.outcome == mailrs_inbound::Outcome::Fail)
            .map(|l| format!("layer:{}", l.name))
            .collect(),
        score: v.score,
        rules_version: v.rules_version,
        judged_at: arrived,
        holds: v.quarantined,
        screening: Vec::new(),
    }
}

/// A verdict the fraud rules gave just now, with what screening matched.
pub fn judged_verdict(
    findings: &mailrs_fraud::Findings,
    rules_version: &str,
    screening: Vec<String>,
) -> LedgerVerdict {
    LedgerVerdict {
        rules: findings.rules().iter().map(|r| r.to_string()).collect(),
        score: findings.score(),
        rules_version: rules_version.to_string(),
        judged_at: chrono::Utc::now().timestamp(),
        holds: mailrs_inbound::holds(findings),
        screening,
    }
}

/// Whether `user` wrote this message, the way delivery decides it.
pub fn is_own(user: &str, raw: &[u8]) -> bool {
    let (_, _, _, _, _, from, _) = crate::headers::extract_headers(raw);
    mailrs_rfc5322::list_contains(&from, user)
}

/// Whether the file at `path` is mail that arrived here: not a copy kept in
/// a Sent folder, and carrying a Received line, which delivery always adds.
/// Mail an account sent under another of our names passes [`is_own`] and is
/// caught here.
pub fn was_received(path: &std::path::Path, raw: &[u8]) -> bool {
    let in_sent = path
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case(".Sent"));
    !in_sent && has_received_line(raw)
}

/// Whether `raw` carries a Received line in its header, which delivery
/// always adds.
pub fn has_received_line(raw: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&raw[..raw.len().min(64 * 1024)]);
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            return false;
        }
        if line
            .get(..9)
            .is_some_and(|name| name.eq_ignore_ascii_case("received:"))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod received_tests {
    use super::was_received;
    use std::path::Path;

    const INBOX: &str = "/data/maildir/golia.jp/u/cur/1.M1:2,S";
    const SENT: &str = "/data/maildir/golia.jp/u/.Sent/cur/1.M1:2,S";

    #[test]
    fn delivered_mail_was_received() {
        let raw = b"Received: from mx.example by us; Sat, 1 Nov 2026 10:00:00 +0000\r\nFrom: a@x.example\r\n\r\nbody";
        assert!(was_received(Path::new(INBOX), raw));
        // the header name in any case, after other fields
        let raw = b"X-Spam: no\nRECEIVED: from mx; Sat, 1 Nov 2026 10:00:00 +0000\n\nbody";
        assert!(was_received(Path::new(INBOX), raw));
    }

    #[test]
    fn a_sent_copy_or_mail_never_delivered_was_not() {
        let delivered = b"Received: from mx; Sat, 1 Nov 2026 10:00:00 +0000\r\nFrom: noreply@golia.jp\r\n\r\nbody";
        assert!(!was_received(Path::new(SENT), delivered));
        let sent = b"Date: Sat, 1 Nov 2026 10:00:00 +0000\r\nFrom: noreply@golia.jp\r\n\r\nbody";
        assert!(!was_received(Path::new(INBOX), sent));
        // a Received line in the body is not a header
        let body = b"From: a@x.example\r\n\r\nReceived: from mx; Sat, 1 Nov 2026\r\n";
        assert!(!was_received(Path::new(INBOX), body));
        // a header line that is short or not ASCII does not stop the scan
        let odd = "ab件名件: y\nX: y\nReceived: from mx; Sat, 1 Nov 2026 10:00:00 +0000\n\nb";
        assert!(was_received(Path::new(INBOX), odd.as_bytes()));
    }
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
            from_reg: mailrs_reputation::registrable(domain),
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
        assert_eq!(
            indexed_link_hosts(&f),
            vec![IndexedLink {
                host: "pay-here.example".into(),
                reg: "pay-here.example".into(),
            }]
        );
    }

    #[test]
    fn link_rows_are_kept_for_the_retention_from_arrival() {
        let f = facts("shop.example", &[], 1_000_000);
        assert_eq!(links_until(&f), 1_000_000 + link_days() * 86_400);
    }

    const US: Option<&str> = Some("mail.golia.ai");
    const PASS: &str = "Authentication-Results: mail.golia.ai; spf=pass; dkim=pass; dmarc=pass\r\n";
    const TRACE: &str = "Received: from mx.shop.example (1.2.3.4:5)\r\n\tby mail.golia.ai with ESMTP\r\n\tfor <u@x>; Mon, 2 Nov 2026 00:00:00 +0000\r\n";
    const REST: &str = "From: Shop <news@shop.example>\r\nSubject: x\r\n\r\nbody\r\n";

    /// The ledger records no results from a header in our name that our
    /// receiver never wrote, and what it wrote as before.
    #[test]
    fn only_our_receivers_results_are_recorded() {
        let forged = format!("{PASS}{REST}");
        assert_eq!(facts_of("u@x", "m@x", forged.as_bytes(), US).auth, "//");
        let ours = format!("{PASS}{TRACE}{REST}");
        assert_eq!(
            facts_of("u@x", "m@x", ours.as_bytes(), US).auth,
            "pass/pass/pass"
        );
    }
}
