//! `dkim_aligned` from the per-signature results this server writes:
//! a sending service's second signature beside the sender's own no
//! longer hides that the sender's verified.
use mailrs_fraud::Policy;
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules, stored::Reading};

const OURS: &str = "mail.golia.ai";

fn message(results: &[&str], signers: &[&str]) -> Vec<u8> {
    let mut raw = String::new();
    for r in results {
        raw.push_str(&format!("Authentication-Results: {r}\r\n"));
    }
    for d in signers {
        raw.push_str(&format!(
            "DKIM-Signature: v=1; a=rsa-sha256; d={d}; s=s1; h=from; bh=x; b=y\r\n"
        ));
    }
    raw.push_str("From: Stripe <invoice@e.stripe.com>\r\nReply-To: billing@runpod.io\r\nSubject: receipt\r\n\r\nthanks\r\n");
    raw.into_bytes()
}

fn aligned(raw: &[u8]) -> bool {
    let probe = format!(
        "{DEFAULT_SOURCE}\nrule('probe-aligned', 'content', 1, false, function(m) if m.dkim_aligned then return 'aligned' end end)\n"
    );
    let reading = Reading::of(raw, Some(OURS), &[]);
    let facts = reading.facts(0, 0, Default::default());
    let result = Rules::compile(&probe)
        .unwrap()
        .classify(&facts, &Policy::default());
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    result.findings.has("probe-aligned")
}

#[test]
fn the_senders_own_pass_beside_a_service_signature_is_aligned() {
    let raw = message(
        &[
            "mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=pass header.d=stripe.com header.s=s1;\r\n\tdkim=pass header.d=amazonses.com header.s=ug7;\r\n\tarc=none;\r\n\tdmarc=pass",
        ],
        &["stripe.com", "amazonses.com"],
    );
    assert!(aligned(&raw));
}

#[test]
fn only_the_services_signature_passing_is_not_aligned() {
    let raw = message(
        &[
            "mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=fail header.d=stripe.com header.s=s1;\r\n\tdkim=pass header.d=amazonses.com header.s=ug7;\r\n\tarc=none;\r\n\tdmarc=fail",
        ],
        &["stripe.com", "amazonses.com"],
    );
    assert!(!aligned(&raw));
}

#[test]
fn per_signature_entries_from_another_server_are_not_read() {
    // the top header is somebody else's; one naming us further down was
    // not put there by us
    let raw = message(
        &[
            "mx.relay.example; dkim=pass header.d=stripe.com",
            "mail.golia.ai; dkim=pass header.d=stripe.com",
        ],
        &["stripe.com", "amazonses.com"],
    );
    assert!(!aligned(&raw));
}

#[test]
fn a_single_word_result_keeps_the_strict_reading() {
    let single = ["mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=pass;\r\n\tarc=none;\r\n\tdmarc=pass"];
    assert!(
        aligned(&message(&single, &["stripe.com"])),
        "every signer from the organisation, and a pass"
    );
    assert!(
        !aligned(&message(&single, &["stripe.com", "amazonses.com"])),
        "with a service's signature beside it, the one word cannot say whose passed"
    );
}
