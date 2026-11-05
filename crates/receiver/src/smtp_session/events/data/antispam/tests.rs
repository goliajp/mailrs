//! The verdict the receiver stores lists every rule that fired, content
//! rules included, each with what it added.

use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RAW: &[u8] = b"From: =?utf-8?B?6b2L6JekIOecnw==?= <ptvmfekd@nogz.cn>\r\n\
To: contact <contact@golia.jp>\r\nSubject: hello\r\n\r\nbody\r\n";

fn findings(asks_for_the_readers_line: bool) -> mailrs_fraud::Findings {
    let facts = Facts {
        from: "ptvmfekd@nogz.cn",
        subject: "hello",
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        asks_for_the_readers_line,
        ..Facts::default()
    };
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let out = rules.classify(&facts, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

fn stored(findings: mailrs_fraud::Findings) -> Option<serde_json::Value> {
    let ctx = mailrs_inbound::ReceiveContext::new(
        "192.0.2.1".parse().expect("ip"),
        "mx.nogz.cn",
        "ptvmfekd@nogz.cn",
        "contact@golia.jp",
        RAW.to_vec(),
        "mx.golia.ai",
    )
    .with_fraud(findings);
    let decision = mailrs_inbound::make_delivery_decision(&ctx.to_pipeline_input(5.0));
    super::fraud_verdict_json(&ctx, 5.0, "test.1", &decision)
        .map(|json| serde_json::from_str(&json).expect("the stored verdict is JSON"))
}

fn content(v: &serde_json::Value) -> &serde_json::Value {
    v["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|l| l["name"] == "content")
        .expect("a content layer")
}

#[test]
fn a_content_rule_is_stored_as_its_own_scored_line() {
    let v = stored(findings(true)).expect("a finding records a verdict");
    let content = content(&v);
    let line = content["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .find(|r| r["code"] == "asks-for-the-readers-line")
        .unwrap_or_else(|| panic!("no line for the rule: {content}"));
    assert_eq!(line["score"], content["score"]);
    assert!(content["score"].as_f64().is_some_and(|s| s > 0.0));
    let lines: f64 = v["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .flat_map(|l| l["reasons"].as_array().expect("reasons"))
        .filter_map(|r| r["score"].as_f64())
        .sum();
    let total = v["score"].as_f64().expect("score");
    assert!((total - lines).abs() < 1e-9, "{v}");
}

/// The counter-example: a message with no content rule firing stores no
/// content line, and a message with no finding stores no verdict.
#[test]
fn no_content_rule_no_content_line() {
    // other rules may read this address; none of them is a content rule
    if let Some(v) = stored(findings(false)) {
        assert!(
            content(&v)["reasons"]
                .as_array()
                .expect("reasons")
                .is_empty(),
            "{v}"
        );
    }
    let mut only_identity = mailrs_fraud::Findings::new();
    only_identity.push(mailrs_fraud::Finding::new(
        "claims-our-name",
        mailrs_fraud::Layer::Identity,
        4.0,
        "display name claims this organisation",
    ));
    let v = stored(only_identity).expect("a finding records a verdict");
    assert!(
        content(&v)["reasons"]
            .as_array()
            .expect("reasons")
            .is_empty(),
        "{v}"
    );
    assert_eq!(content(&v)["score"], 0.0);
}
