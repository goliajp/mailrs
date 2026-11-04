//! A message's template fingerprint, built the same way by both cores.

use mailrs_report::Template;

/// The strings that name the reader, taken out of the body before it is
/// hashed: the account's address and local part, and every address and
/// display name in `To:` and `Cc:` (mail personalised for each recipient
/// often greets them there).
pub(crate) fn recipient_names(user: &str, raw: &[u8]) -> Vec<String> {
    let mut out = vec![user.to_ascii_lowercase()];
    if let Some((local, _)) = user.split_once('@') {
        out.push(local.to_ascii_lowercase());
    }
    for h in ["to", "cc"] {
        let Some(v) = crate::dsn::header_value(raw, h) else {
            continue;
        };
        let v = mailrs_rfc2047::decode(v.as_bytes()).into_owned();
        for mailbox in mailrs_rfc5322::split_list(&v) {
            let addr = mailrs_rfc5322::addr_spec(mailbox).to_ascii_lowercase();
            if let Some((local, _)) = addr.split_once('@') {
                out.push(local.to_string());
            }
            out.push(addr);
            let name = mailbox
                .split('<')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .trim();
            if !name.is_empty() && !name.contains('@') {
                out.push(name.to_string());
                out.extend(name.split_whitespace().map(str::to_string));
            }
        }
    }
    out
}

/// The fingerprint of `raw` as `user` received it: the body's SimHash and
/// the link domains, attachments and sender domain it is compared on.
/// `None` when the body is too short to compare.
pub fn template_of(user: &str, raw: &[u8]) -> Option<Template> {
    let names = recipient_names(user, raw);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let simhash = mailrs_report::fingerprint::of_message(raw, &names)?;
    let parsed = mailrs_mime::parse(raw);
    let mut link_domains: Vec<String> = mailrs_inbound::scan_input::link_hosts(&parsed)
        .iter()
        .map(|h| mailrs_reputation::registrable(h))
        .collect();
    link_domains.sort();
    link_domains.dedup();
    let attachments = mailrs_inbound::scan_input::attachments(&parsed)
        .iter()
        .map(|a| {
            use sha2::Digest as _;
            sha2::Sha256::digest(a.bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        })
        .collect();
    let from = mailrs_inbound::identity::from_address(raw).to_ascii_lowercase();
    let sender_domain = from
        .rsplit_once('@')
        .map(|(_, d)| mailrs_reputation::registrable(d))
        .unwrap_or_default();
    Some(Template {
        simhash,
        link_domains,
        attachments,
        sender_domain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reader_is_named_by_address_local_part_and_display_name() {
        let raw = b"From: x@evil.example\r\nTo: \"Alice Smith\" <alice@a.example>, bob@b.example\r\n\r\nbody";
        let names = recipient_names("alice@a.example", raw);
        for want in [
            "alice@a.example",
            "alice",
            "Alice Smith",
            "Smith",
            "bob@b.example",
            "bob",
        ] {
            assert!(
                names.iter().any(|n| n == want),
                "{want} missing from {names:?}"
            );
        }
    }

    #[test]
    fn copies_to_two_people_share_their_fingerprint_and_facts() {
        let body = |name: &str, addr: &str| {
            format!(
                "From: Desk <notice@parcel-desk.example>\r\nTo: {name} <{addr}>\r\nSubject: parcel\r\n\r\n\
                 Dear {name}, your parcel could not be delivered. Pay the customs fee at \
                 https://parcel-desk.example/pay?to={addr} within 24 hours, otherwise it is returned.\r\n"
            )
        };
        let a = template_of(
            "alice@a.example",
            body("Alice", "alice@a.example").as_bytes(),
        )
        .unwrap();
        let b = template_of("bob@b.example", body("Bob", "bob@b.example").as_bytes()).unwrap();
        assert!(
            mailrs_report::fingerprint::distance(a.simhash, b.simhash)
                <= mailrs_report::MAX_DISTANCE
        );
        assert_eq!(a.link_domains, vec!["parcel-desk.example".to_string()]);
        assert_eq!(a.sender_domain, "parcel-desk.example");
        assert!(mailrs_report::same(&a, &b).is_some());
    }
}
