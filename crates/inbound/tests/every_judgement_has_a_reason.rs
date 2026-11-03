//! Every judgement the receive path makes about a message carries a
//! reason the reader can be shown, in every language the clients speak.
//!
//! One case per way a message reaches Junk, and per kind of finding a
//! verdict layer can fail on. A new Junk branch without a reason code,
//! or a code without a sentence, fails here.

use std::collections::HashSet;

use mailrs_inbound::reason::TABLE;
use mailrs_inbound::{
    AuthResults, DeliveryDecision, DmarcPolicy, Outcome, PipelineInput, Reason, assess,
    make_delivery_decision,
};

fn input() -> PipelineInput {
    PipelineInput {
        greylisted: false,
        auth: AuthResults {
            spf: "pass".into(),
            dkim: "pass".into(),
            arc: "none".into(),
            dmarc: "pass".into(),
            dmarc_policy: DmarcPolicy::Pass,
            dkim_signatures: Vec::new(),
        },
        virus_found: None,
        content_score: 0.0,
        matched_rules: vec![],
        ptr_score: 0.0,
        ai_score: 0.0,
        deception: Default::default(),
        fraud: mailrs_fraud::Findings::new(),
        spam_threshold: 5.0,
        hostname: "mx.golia.ai".into(),
        from_addr: "someone@elsewhere.example".into(),
        header_from_addr: "someone@elsewhere.example".into(),
        recipient_whitelist: HashSet::new(),
        recipient_blacklist: HashSet::new(),
        local_domains: HashSet::new(),
        sender_allowed: false,
        own_mail: false,
    }
}

fn worded(r: &Reason) {
    let table: serde_json::Value = serde_json::from_str(TABLE).unwrap();
    for lang in ["en", "zh", "ja"] {
        let text = table[&r.code][lang]
            .as_str()
            .unwrap_or_else(|| panic!("{} has no {lang} sentence", r.code));
        for (name, _) in text.match_indices('{') {
            let rest = &text[name + 1..];
            let param = rest.split('}').next().unwrap();
            assert!(
                r.params.contains_key(param),
                "{}: the {lang} sentence names {{{param}}}, which the reason does not carry",
                r.code
            );
        }
    }
}

fn junk_reason(input: &PipelineInput) -> Reason {
    match make_delivery_decision(input) {
        DeliveryDecision::Junk { why, .. } => why,
        other => panic!("expected Junk, got {other:?}"),
    }
}

#[test]
fn claiming_our_domain_and_failing_dmarc() {
    let mut i = input();
    i.local_domains.insert("golia.jp".into());
    i.header_from_addr = "ceo@golia.jp".into();
    i.auth.dmarc = "fail".into();
    let r = junk_reason(&i);
    assert_eq!(r.code, "junk-claims-our-domain");
    worded(&r);
}

#[test]
fn the_recipients_blocklist() {
    let mut i = input();
    i.recipient_blacklist
        .insert("someone@elsewhere.example".into());
    let r = junk_reason(&i);
    assert_eq!(r.code, "junk-your-blocklist");
    assert_eq!(r.params["sender"], "someone@elsewhere.example");
    worded(&r);
}

#[test]
fn the_senders_dmarc_policy() {
    let mut i = input();
    i.auth.dmarc = "fail".into();
    i.auth.dmarc_policy = DmarcPolicy::Quarantine;
    let r = junk_reason(&i);
    assert_eq!(r.code, "junk-dmarc-quarantine");
    worded(&r);
}

#[test]
fn the_score_past_the_threshold_names_what_it_added_up() {
    let mut i = input();
    i.content_score = 3.0;
    i.ai_score = 2.5;
    let r = junk_reason(&i);
    assert_eq!(r.code, "junk-score");
    assert_eq!(r.params["score"], "5.5");
    assert_eq!(r.params["threshold"], "5.0");
    assert_eq!(r.params["classifier"], "2.5");
    worded(&r);
}

/// Accepted mail has no Junk reason, so a test that passed because every
/// decision carried one regardless would fail here.
#[test]
fn accepted_mail_is_not_given_a_junk_reason() {
    assert!(matches!(
        make_delivery_decision(&input()),
        DeliveryDecision::Accept { .. }
    ));
}

/// Every layer that failed says what it failed on.
#[test]
fn every_failing_layer_carries_reasons() {
    let mut i = input();
    i.auth.dmarc = "fail".into();
    i.content_score = 2.0;
    i.matched_rules = vec!["list-id".into()];
    i.deception.unjustified_zero_width = true;
    i.fraud = mailrs_fraud::findings_for(&["claims-our-name", "x-mailer-generated"]);
    let v = assess(&i);
    let mut failed = 0;
    for layer in &v.layers {
        if layer.outcome == Outcome::Fail {
            failed += 1;
            assert!(
                !layer.reasons.is_empty(),
                "{} failed with no reason",
                layer.name
            );
        }
        for r in &layer.reasons {
            worded(r);
        }
    }
    assert_eq!(failed, 4, "the case should fail every layer: {v:?}");
    let identity = v.layers.iter().find(|l| l.name == "identity").unwrap();
    let codes: Vec<&str> = identity.reasons.iter().map(|r| r.code.as_str()).collect();
    assert!(codes.contains(&"claims-our-name"), "{codes:?}");
    assert!(codes.contains(&"zero-width-padding"), "{codes:?}");
}

/// A clean message's rule layers say nothing; transport still lists what
/// each check said, because "checked and passed" is what the reader needs.
#[test]
fn a_clean_message_lists_its_checks_and_nothing_else() {
    let v = assess(&input());
    for layer in &v.layers {
        match layer.name.as_str() {
            "transport" => assert_eq!(layer.reasons.len(), 3),
            _ => assert!(
                layer.reasons.is_empty(),
                "{}: {:?}",
                layer.name,
                layer.reasons
            ),
        }
    }
}
