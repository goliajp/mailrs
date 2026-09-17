//! Frozen Rust implementation is the migration oracle, not the serving path.
use mailrs_fraud::{Facts, Findings, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

fn comparable(f: &Findings) -> Vec<(String, String, u64, bool, String)> {
    let mut rows: Vec<_> = f
        .iter()
        .map(|f| {
            (
                f.rule.clone(),
                f.layer.as_str().into(),
                f.score.to_bits(),
                f.holds,
                f.detail.clone(),
            )
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn all_existing_sender_fixtures_match_compiled_rules() {
    let senders: Vec<String> = serde_json::from_str(include_str!("senders.json")).unwrap();
    let policy = Policy {
        org_names: vec!["GOLIA株式会社".into(), "GOLIA K.K.".into()],
        our_domains: vec!["golia.jp".into(), "golia.ai".into()],
        allowed_domains: vec![
            "slack.com".into(),
            "github.com".into(),
            "atlassian.net".into(),
        ],
        account_names: vec!["LI HAO".into(), "No Reply".into(), "李好".into()],
    };
    let mut rules = Rules::compile(DEFAULT_SOURCE).unwrap();
    // Sixteen migrated rules plus `claims-our-domain`, which was
    // written after the migration and has no Rust counterpart to
    // compare against — the oracle below is frozen on purpose.  It
    // cannot fire here either: it needs the `unauthenticated` fact,
    // which these Facts leave false.  Its own tests are in
    // `tests/our_domain.rs`.
    assert_eq!(rules.rule_count(), 17);
    for from in &senders {
        for (subject, seen, to) in [
            (
                "[最終リマインダー]: お支払い方法を更新してください。",
                0,
                "",
            ),
            ("【重要】Amazonプライム：支払い方法未更新", 3, ""),
            ("GOLIA株式会社 業務指示", 100, ""),
            ("Re: RE：年末調整の件（GOLIA株式会社）", 0, ""),
            ("兰静思，您2026喜逢财星苏醒", 0, "LI HAO"),
            ("李好，添加Mikio Yamaguchi", 0, "李好"),
            ("哈喽，是多儿。", 0, ""),
            ("【OpenAI】payment failed", 0, ""),
        ] {
            let domain = mailrs_fraud::impersonation::address_of(from)
                .and_then(|s| s.rsplit('@').next())
                .unwrap_or("");
            let f = Facts {
                from,
                domain,
                subject,
                domain_seen: seen,
                to_display: to,
                ..Facts::default()
            };
            let expected = mailrs_fraud::scan(&f, &policy);
            let actual = rules.classify(&f, &policy);
            assert!(actual.errors.is_empty(), "{:?}", actual.errors);
            assert_eq!(
                comparable(&actual.findings),
                comparable(&expected),
                "from={from}, subject={subject}, seen={seen}"
            );
        }
    }
}

#[test]
fn all_sixteen_rules_have_positive_parity_coverage() {
    let mut rules = Rules::compile(DEFAULT_SOURCE).unwrap();
    let p = Policy {
        org_names: vec!["GOLIA株式会社".into()],
        account_names: vec!["LI HAO".into()],
        ..Policy::default()
    };
    let mut covered = std::collections::BTreeSet::new();
    for from in [
        "ChatGPT <a@unrelated.example>",
        "GOLIA株式会社 <a@unrelated.example>",
        "LI HAO <a@unrelated.example>",
        "<omqqy@wzglff.com>",
    ] {
        for domain in ["aliyun.rvezovp.cn", "mta176.geimiu.com"] {
            for subject in ["【Amazon】支払い", "GOLIA株式会社", "兰静思，您好"] {
                let f = Facts {
                    from,
                    domain,
                    subject,
                    x_mailer: Some("phevb tmiyui 191.8187.55074"),
                    reply_rotation: 4,
                    has_zero_width_in_name: true,
                    has_zero_width_inside_a_word: true,
                    has_bidi_override: true,
                    has_executable_attachment: true,
                    ..Facts::default()
                };
                let actual = rules.classify(&f, &p);
                assert!(actual.errors.is_empty(), "{:?}", actual.errors);
                assert_eq!(
                    comparable(&actual.findings),
                    comparable(&mailrs_fraud::scan(&f, &p))
                );
                covered.extend(actual.findings.rules().into_iter().map(str::to_owned));
            }
        }
    }
    // The migrated sixteen, each fired at least once and each matching
    // the frozen Rust oracle.  Rules added after the migration are not
    // in this matrix and must not be: there is nothing to compare them
    // with.
    assert_eq!(covered.len(), 16, "{covered:?}");
}

#[test]
fn exhausted_or_throwing_rule_does_not_prevent_the_next_rule() {
    for broken in ["while true do end", "error('broken')"] {
        let source = format!(
            "rule('bad','identity',1,true,function(m) {broken} end)\nrule('good','content',6,true,function(m) return 'caught' end)"
        );
        let mut rules = Rules::compile(&source).unwrap();
        for _ in 0..2 {
            let r = rules.classify(&Facts::default(), &Policy::default());
            assert_eq!(r.errors.len(), 1);
            assert!(r.findings.has("good"));
        }
    }
}

#[test]
fn fifty_thousand_messages_reset_budget_and_do_not_accumulate_roots() {
    let mut rules =
        Rules::compile("rule('test','identity',1,false,function(m) return m.subject end)").unwrap();
    let f = Facts {
        subject: "same VM, new message",
        ..Facts::default()
    };
    for _ in 0..50_000 {
        let r = rules.classify(&f, &Policy::default());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.findings.len(), 1);
    }
}

#[test]
fn sandbox_and_registration_are_validated() {
    for source in [
        "while true do end",
        "rule('x','invalid',1,true,function() end)",
        "rule('x','identity',-1,true,function() end)",
        "return io.open('/etc/passwd')",
        "return os.execute('true')",
        "return require('io')",
        "return load('return 1')",
        "rule('x','identity',1,true,function() end);rule('x','identity',1,true,function() end)",
    ] {
        assert!(Rules::compile(source).is_err(), "{source}");
    }
}
