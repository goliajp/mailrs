//! The rules' input for a message already on disk — re-played from the
//! store, or brought in by an import — built the way the receiver builds
//! it for one arriving over SMTP.
//!
//! What the receiver knows from the session, this reads back off the
//! message: the client address from the first `Received:` line, the
//! authentication results from `Authentication-Results:`. The envelope
//! domain comes from `Return-Path:` and is passed in, so each caller reads
//! that header the way it already does.

/// Build the input for `raw` and hand it to `run`, which evaluates it with
/// whichever rule set it holds. `own` is this deployment's domains,
/// lower-case: a sender is from outside unless it claims one of them and
/// passed DMARC. `authserv` is this server's authserv-id: a results header
/// in that name its receiver did not write is not read.
pub fn with_input<R>(
    set: &mailrs_reputation::Set,
    inspector: &mailrs_attachment_scan::Inspector,
    own: &[String],
    raw: &[u8],
    envelope_domain: &str,
    authserv: Option<&str>,
    run: impl FnOnce(&crate::Input<'_>) -> R,
) -> R {
    let parsed = mailrs_mime::parse(raw);
    let (spf, _, dmarc) = mailrs_inbound::identity::auth_results_tokens_trusted(raw, authserv);
    let from = mailrs_inbound::identity::from_address(raw).to_ascii_lowercase();
    let header_domain = from
        .rsplit_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_default();
    let m = mailrs_reputation::Message {
        client_ip: mailrs_inbound::identity::first_received_peer_ip(raw),
        envelope_domain: envelope_domain.to_string(),
        header_domain: header_domain.clone(),
        link_hosts: mailrs_inbound::scan_input::link_hosts(&parsed),
        auth: mailrs_reputation::Auth {
            dmarc_fail: dmarc.eq_ignore_ascii_case("fail"),
            spf_fail: spf.eq_ignore_ascii_case("fail"),
        },
    };
    let reputation = set.facts(&m);
    let parts = mailrs_inbound::scan_input::attachments(&parsed);
    let atts: Vec<mailrs_attachment_scan::Attachment> = parts
        .iter()
        .map(|p| mailrs_attachment_scan::Attachment {
            name: &p.name,
            declared_type: &p.declared_type,
            bytes: p.bytes,
        })
        .collect();
    let parts = inspector.inspect(&atts);
    let ours = own.contains(&header_domain) && dmarc.eq_ignore_ascii_case("pass");
    run(&crate::Input {
        outside: !ours,
        parts: &parts,
        reputation: &reputation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const US: Option<&str> = Some("mail.golia.ai");
    const PASS: &str = "Authentication-Results: mail.golia.ai; spf=pass; dkim=pass; dmarc=pass\r\n";
    const TRACE: &str = "Received: from mx.golia.jp (1.2.3.4:5)\r\n\tby mail.golia.ai with ESMTP\r\n\tfor <a@golia.jp>; Mon, 2 Nov 2026 00:00:00 +0000\r\n";
    const REST: &str = "From: Ops <ops@golia.jp>\r\nSubject: x\r\n\r\nbody\r\n";

    fn outside(raw: &str) -> bool {
        let set = mailrs_reputation::Set::build(&[], &mailrs_reputation::NeverList::default(), 0);
        let inspector = mailrs_attachment_scan::Inspector::build(
            &[],
            mailrs_attachment_scan::Limits::default(),
            0,
        );
        let own = vec!["golia.jp".to_string()];
        with_input(&set, &inspector, &own, raw.as_bytes(), "", US, |i| {
            i.outside
        })
    }

    /// One of our domains in `From:` with a DMARC pass in a header our
    /// receiver never wrote is still from outside.
    #[test]
    fn a_pass_in_our_name_without_our_trace_is_not_ours() {
        assert!(outside(&format!("{PASS}{REST}")));
    }

    /// What our receiver delivered reads as it did.
    #[test]
    fn our_receivers_pass_still_makes_it_ours() {
        assert!(!outside(&format!("{PASS}{TRACE}{REST}")));
    }
}
