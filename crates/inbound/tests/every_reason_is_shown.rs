//! A reader looking at why a message is in Junk or held sees every
//! reason, each with what it added, and the numbers add up.
//!
//! Content-layer rule findings were counted toward Junk and never
//! listed: the content layer said "nothing found" over a letter the
//! content rules had held.

use std::collections::HashSet;

use mailrs_fraud::{Finding, Findings, Layer};
use mailrs_inbound::{
    AuthResults, DeliveryDecision, DmarcPolicy, FraudVerdict, Outcome, PipelineInput, assess,
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
        fraud: Findings::new(),
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

fn layer<'a>(v: &'a FraudVerdict, name: &str) -> &'a mailrs_inbound::Layer {
    v.layers.iter().find(|l| l.name == name).expect(name)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// Every source of score at once: the content scan, the classifier, a
/// rule under each layer including transport, and both host signals.
fn everything() -> PipelineInput {
    let mut i = input();
    i.auth.dmarc = "fail".into();
    i.content_score = 2.0;
    i.ai_score = 1.25;
    i.matched_rules = vec!["urgent_wire".into()];
    i.deception.unjustified_zero_width = true;
    i.fraud = mailrs_fraud::findings_for(&["claims-our-name", "x-mailer-generated"]);
    i.fraud.push(Finding::scored(
        "offers-the-reader-a-sum",
        Layer::Content,
        5.0,
        "the text offers the reader a share of a sum in the millions",
    ));
    i.fraud.push(Finding::new(
        "asks-for-the-readers-line",
        Layer::Content,
        6.0,
        "the text asks the reader to send their own LINE QR code",
    ));
    i.fraud.push(Finding::scored(
        "lua:transport-rule",
        Layer::Transport,
        1.5,
        "a rule filed under transport",
    ));
    i
}

/// A content rule is a line of its own, with its score, and the layer
/// fails on it.
#[test]
fn a_content_rule_is_listed_with_what_it_added() {
    let mut i = input();
    i.fraud.push(Finding::new(
        "asks-for-the-readers-line",
        Layer::Content,
        6.0,
        "the text asks the reader to send their own LINE QR code",
    ));
    let v = assess(&i);
    let content = layer(&v, "content");
    assert_eq!(content.outcome, Outcome::Fail);
    assert!(close(content.score, 6.0), "{content:?}");
    let line = content
        .reasons
        .iter()
        .find(|r| r.code == "asks-for-the-readers-line")
        .expect("the rule's own line");
    assert_eq!(line.score, Some(6.0));
    assert!(
        content.detail.contains("LINE QR code"),
        "{}",
        content.detail
    );
    assert!(close(v.score, 6.0));
}

/// Each layer's score is the sum of the lines it lists, the verdict's is
/// the sum of the layers, and that is the total the Junk reason names.
/// Nothing counts that is not shown.
#[test]
fn every_score_is_the_sum_of_the_lines_shown() {
    let i = everything();
    let v = assess(&i);
    for l in &v.layers {
        let lines: f64 = l.reasons.iter().filter_map(|r| r.score).sum();
        assert!(
            close(l.score, lines),
            "{}: layer {} but lines add up to {lines}: {:?}",
            l.name,
            l.score,
            l.reasons
        );
    }
    let layers: f64 = v.layers.iter().map(|l| l.score).sum();
    assert!(close(v.score, layers));

    let why = match make_delivery_decision(&i) {
        DeliveryDecision::Junk { why, .. } => why,
        other => panic!("expected Junk, got {other:?}"),
    };
    assert_eq!(why.params["score"], format!("{:.1}", v.score));

    let codes: Vec<&str> = layer(&v, "content")
        .reasons
        .iter()
        .map(|r| r.code.as_str())
        .collect();
    assert_eq!(
        codes,
        [
            "content-score",
            "offers-the-reader-a-sum",
            "asks-for-the-readers-line"
        ]
    );
    let transport = layer(&v, "transport");
    assert!(close(transport.score, 1.5));
    assert!(
        transport
            .reasons
            .iter()
            .any(|r| r.code == "lua:transport-rule")
    );
}

/// Every line of a scoring layer says what it added; only the three
/// authentication checks, which add nothing, leave it out.
#[test]
fn only_the_authentication_checks_carry_no_score() {
    let v = assess(&everything());
    for l in &v.layers {
        for r in &l.reasons {
            match r.code.as_str() {
                "auth-check" => assert_eq!(r.score, None),
                _ => assert!(r.score.is_some_and(|s| s > 0.0), "{}: {r:?}", l.name),
            }
        }
    }
}

/// The counter-example: with no content rule firing, the content layer
/// lists nothing and adds nothing, and a stored line carries no score
/// field unless it scored.
#[test]
fn without_a_content_rule_the_content_layer_is_empty() {
    let mut i = input();
    i.fraud = mailrs_fraud::findings_for(&["claims-our-name"]);
    let v = assess(&i);
    let content = layer(&v, "content");
    assert_eq!(content.outcome, Outcome::Pass);
    assert!(content.reasons.is_empty(), "{:?}", content.reasons);
    assert_eq!(content.score, 0.0);

    let json: serde_json::Value = serde_json::to_value(&v).expect("serialise");
    let transport = &json["layers"][0];
    assert_eq!(transport["name"], "transport");
    assert!(
        transport["reasons"][0].get("score").is_none(),
        "{transport}"
    );
    assert!(json["layers"][1]["reasons"][0]["score"].is_number());
}

/// A verdict stored before lines carried a score still reads.
#[test]
fn a_verdict_stored_before_lines_were_scored_reads() {
    let old = r#"{"rules_version":"2026-08-29.1","score":4.0,"quarantined":true,
        "layers":[{"name":"identity","outcome":"fail","score":4.0,"detail":"d",
        "reasons":[{"code":"claims-our-name"}]}]}"#;
    let v: FraudVerdict = serde_json::from_str(old).expect("reads");
    assert_eq!(v.layers[0].reasons[0].score, None);
}
