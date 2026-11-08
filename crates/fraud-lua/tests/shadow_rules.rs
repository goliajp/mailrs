//! Shadow rules run and are counted, and never score or hold.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};
use std::{fs, process::Command};

const ALWAYS: &str = "function(m) return 'probe' end";

fn bundle(extra: &str) -> String {
    format!("{DEFAULT_SOURCE}\n{extra}\n")
}

fn facts() -> Facts<'static> {
    Facts {
        from: "ChatGPT <admin@heavenerandassociates.com>",
        ..Facts::default()
    }
}

#[test]
fn a_shadow_hit_adds_no_score_and_holds_nothing() {
    let plain = Rules::compile(&bundle(&format!(
        "rule('probe', 'content', 50, false, {ALWAYS})"
    )))
    .unwrap()
    .classify(&Facts::default(), &Policy::default());
    assert!(plain.findings.has("probe"), "the probe fires");
    let mut shadowed = Rules::compile(&bundle(&format!(
        "rule('probe', 'content', 50, false, {ALWAYS}, {{shadow = true}})"
    )))
    .unwrap();
    let clean = shadowed.classify(&Facts::default(), &Policy::default());
    assert!(clean.errors.is_empty(), "{:?}", clean.errors);
    assert!(
        !clean.findings.has("probe"),
        "a shadow hit is not a finding"
    );
    assert!(clean.shadow.has("probe"), "but it is recorded");
    assert_eq!(clean.findings.score(), 0.0);
    assert!(!clean.findings.hold_worthy());

    // on a message the other rules hold, the shadow rule changes nothing
    let baseline = Rules::compile(DEFAULT_SOURCE)
        .unwrap()
        .classify(&facts(), &Policy::default());
    let with_shadow = shadowed.classify(&facts(), &Policy::default());
    assert!(baseline.findings.hold_worthy(), "the sample is held");
    assert_eq!(with_shadow.findings.score(), baseline.findings.score());
    assert_eq!(
        with_shadow.findings.hold_worthy(),
        baseline.findings.hold_worthy()
    );
    assert_eq!(with_shadow.findings.rules(), baseline.findings.rules());
    assert!(with_shadow.shadow.has("probe"));
}

#[test]
fn a_shadow_rule_that_holds_does_not_compile() {
    let Err(error) = Rules::compile(&bundle(&format!(
        "rule('probe', 'content', 5, true, {ALWAYS}, {{shadow = true}})"
    ))) else {
        panic!("a holding shadow rule compiled");
    };
    assert!(error.contains("a shadow rule cannot hold"), "{error}");
}

#[test]
fn an_unknown_rule_option_does_not_compile() {
    for opts in ["{shadow = 1}", "{shaddow = true}", "'shadow'"] {
        let source = bundle(&format!(
            "rule('probe', 'content', 5, false, {ALWAYS}, {opts})"
        ));
        assert!(
            Rules::compile(&source).is_err(),
            "options {opts} were accepted"
        );
    }
}

#[test]
fn the_shadow_flag_alone_changes_the_version() {
    let plain = Rules::compile(&bundle(&format!(
        "rule('probe', 'content', 5, false, {ALWAYS})"
    )))
    .unwrap();
    let shadowed = Rules::compile(&bundle(&format!(
        "rule('probe', 'content', 5, false, {ALWAYS}, {{shadow = true}})"
    )))
    .unwrap();
    assert_ne!(plain.version(), shadowed.version());
    assert!(!plain.shadow_rules().contains(&"probe".to_string()));
    assert!(shadowed.shadow_rules().contains(&"probe".to_string()));
}

#[test]
fn the_enforced_rules_are_every_rule_but_the_shadow_ones() {
    let rules = Rules::compile(DEFAULT_SOURCE).unwrap();
    let enforced = rules.enforced_rules();
    assert_eq!(
        enforced.len() + rules.shadow_rules().len(),
        rules.rule_count()
    );
    for shadow in rules.shadow_rules() {
        assert!(!enforced.contains(shadow), "{shadow} is shadow");
    }
    for id in [
        "attachment-is-a-login-page",
        "html-builds-a-download",
        "svg-runs-script",
        "bec-pay-to-a-lookalike",
        "bec-pay-to-a-stranger",
        "new-domain-asks-for-payment",
        "new-domain-login-link",
    ] {
        assert!(enforced.contains(&id.to_string()), "{id} is enforced");
    }
}

/// `bec-name-borrowed` left shadow in production's pack once the
/// correspondent facts cut its hits to one campaign; it scores and never
/// holds. The other two still run in shadow, and so do the Japanese
/// account and account-change rules until their review, the QR rules, and
/// the rule for unvouched senders linking only to a generated bucket.
#[test]
fn four_bec_rules_the_qr_rules_and_the_bucket_rule_ship_in_shadow() {
    let rules = Rules::compile(DEFAULT_SOURCE).unwrap();
    assert_eq!(
        rules.shadow_rules(),
        [
            "unvouched-sender-links-only-to-a-bucket",
            "bec-lookalike-reply-diverted",
            "bec-reply-diverted-unauthenticated",
            "bec-jp-account-from-a-stranger",
            "bec-account-changed",
            "qr-text-says-another-domain",
            "qr-only-way-out",
            "qr-to-a-new-domain"
        ]
    );
    assert!(
        rules
            .enforced_rules()
            .contains(&"bec-name-borrowed".to_string())
    );
}

#[test]
fn compare_counts_shadow_hits_apart_from_rule_changes() {
    let root =
        std::env::temp_dir().join(format!("mailrs-fraud-check-shadow-{}", std::process::id()));
    let maildir = root.join("golia.jp/lihao/cur");
    fs::create_dir_all(&maildir).unwrap();
    for i in 0..3 {
        fs::write(
            maildir.join(i.to_string()),
            "From: base2026 <base2026@tintgate.com>\r\nSubject: hello\r\nDate: Mon, 12 Oct 2026 18:40:00 +0000\r\n\r\nHello\r\n",
        )
        .unwrap();
    }
    let candidate = root.join("candidate.lua");
    fs::write(
        &candidate,
        bundle(&format!(
            "rule('probe', 'content', 50, false, {ALWAYS}, {{shadow = true}})"
        )),
    )
    .unwrap();
    let list = root.join("list.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
        .args([
            "compare",
            "builtin",
            candidate.to_str().unwrap(),
            root.join("golia.jp").to_str().unwrap(),
            "3",
            "--list",
            list.to_str().unwrap(),
        ])
        .env_remove("MAILRS_KEVY_URL")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["changed"], 0,
        "a shadow hit is not a change: {report}"
    );
    assert_eq!(report["added_holds"], 0, "{report}");
    assert_eq!(report["shadow_hits_by_rule"]["probe"], 3, "{report}");
    assert_eq!(
        report["baseline_shadow_hits_by_rule"].get("probe"),
        None,
        "{report}"
    );
    assert_eq!(
        report["shadow_hits_by_rule"]["bec-lookalike-reply-diverted"], 0,
        "a shadow rule that never fired is listed at zero: {report}"
    );
    assert!(
        report["shadow_rules"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("probe")),
        "{report}"
    );
    let first: serde_json::Value =
        serde_json::from_str(fs::read_to_string(&list).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(first["shadow_before"], serde_json::json!([]));
    assert_eq!(first["shadow_after"], serde_json::json!(["probe"]));
    assert_eq!(first["rules_before"], first["rules_after"]);
    fs::remove_dir_all(root).unwrap();
}
