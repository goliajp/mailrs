//! `bec-lookalike-reply-diverted` read off whole messages: replies sent to
//! another organisation, judged by the public suffix list, and a sender
//! whose own DKIM speaks for an established domain left alone.
use mailrs_fraud::novelty::Novelty;
use mailrs_fraud::relation::Relation;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::stored::Reading;
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "bec-lookalike-reply-diverted";

fn lookalike() -> Relation {
    Relation {
        looks_like_correspondent: true,
        lookalike_of: Some("correspondent.example".into()),
        ..Relation::default()
    }
}

fn message(from: &str, reply_to: &str, auth: &str, signers: &[&str]) -> Vec<u8> {
    let mut raw = format!("Authentication-Results: mx.ours.test; {auth}\r\n");
    for d in signers {
        raw.push_str(&format!(
            "DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed;\r\n d={d}; s=s1; h=from:subject; bh=AAAA; b=BBBB\r\n"
        ));
    }
    raw.push_str(&format!(
        "From: Events <{from}>\r\nReply-To: {reply_to}\r\nSubject: hello\r\n\r\nbody\r\n"
    ));
    raw.into_bytes()
}

fn fires(raw: &[u8], novelty: Novelty) -> bool {
    let reading = Reading::of(raw, None, &[]);
    let f = Facts {
        novelty,
        ..reading.facts(0, 0, lookalike())
    };
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let out = rules.classify(&f, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    // the rule ships in shadow: a hit is recorded, never scored
    assert!(!out.findings.rules().contains(&RULE));
    out.shadow.rules().contains(&RULE)
}

fn old_domain() -> Novelty {
    Novelty {
        sender_age_days: Some(400),
        sender_seen: 120,
        ..Novelty::default()
    }
}

fn new_domain() -> Novelty {
    Novelty {
        sender_age_days: Some(2),
        sender_seen: 1,
        sender_new: true,
        ..Novelty::default()
    }
}

#[test]
fn replies_to_the_senders_own_organisation_are_not_diverted() {
    for (from, reply) in [
        ("events@mail.nvidia.cn", "events@nvidia.cn"),
        ("mitsuruhiraga@e.stripe.com", "support@stripe.com"),
    ] {
        let raw = message(from, reply, "spf=pass; dkim=none; dmarc=none", &[]);
        assert!(!fires(&raw, Novelty::default()), "{from} -> {reply}");
    }
}

#[test]
fn two_organisations_under_one_multi_label_suffix_are_two() {
    // `com.cn` is a public suffix: shop-a and shop-b are unrelated
    let raw = message(
        "info@shop-a.com.cn",
        "pay@shop-b.com.cn",
        "spf=pass; dkim=none; dmarc=none",
        &[],
    );
    assert!(fires(&raw, Novelty::default()));
}

#[test]
fn a_lookalike_signing_its_own_new_domain_still_counts() {
    let raw = message(
        "info@auabe.com",
        "sales@worktank.example",
        "spf=pass; dkim=pass; dmarc=pass",
        &["auabe.com"],
    );
    assert!(fires(&raw, new_domain()));
}

#[test]
fn an_established_domain_whose_own_dkim_passes_is_left_alone() {
    let raw = message(
        "events@mail.nvidia.cn",
        "events@nvidia-partner.example",
        "spf=pass; dkim=pass; dmarc=pass",
        &["nvidia.cn"],
    );
    assert!(!fires(&raw, old_domain()));
}

#[test]
fn passing_dkim_alone_does_not_skip() {
    let raw = message(
        "events@mail.nvidia.cn",
        "events@nvidia-partner.example",
        "spf=pass; dkim=pass; dmarc=pass",
        &["nvidia.cn"],
    );
    assert!(fires(&raw, new_domain()), "new domain");
    assert!(
        fires(&raw, Novelty::default()),
        "domain history unknown: not established"
    );
}

#[test]
fn a_signature_by_someone_else_is_not_alignment() {
    let raw = message(
        "events@mail.nvidia.cn",
        "events@nvidia-partner.example",
        "spf=pass; dkim=pass; dmarc=pass",
        &["nvidia.cn", "esp-relay.example"],
    );
    assert!(fires(&raw, old_domain()), "which signer passed is unknown");
    let failed = message(
        "events@mail.nvidia.cn",
        "events@nvidia-partner.example",
        "spf=pass; dkim=fail; dmarc=fail",
        &["nvidia.cn"],
    );
    assert!(fires(&failed, old_domain()), "signature did not verify");
}

#[test]
fn either_age_or_volume_establishes_a_domain() {
    let raw = message(
        "events@mail.nvidia.cn",
        "events@nvidia-partner.example",
        "spf=pass; dkim=pass; dmarc=pass",
        &["nvidia.cn"],
    );
    let by_age = Novelty {
        sender_age_days: Some(31),
        sender_seen: 0,
        ..Novelty::default()
    };
    let by_volume = Novelty {
        sender_age_days: Some(3),
        sender_seen: 5,
        ..Novelty::default()
    };
    let neither = Novelty {
        sender_age_days: Some(30),
        sender_seen: 4,
        ..Novelty::default()
    };
    assert!(!fires(&raw, by_age));
    assert!(!fires(&raw, by_volume));
    assert!(fires(&raw, neither));
}
