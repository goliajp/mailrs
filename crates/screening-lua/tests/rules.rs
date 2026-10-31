use mailrs_attachment_scan::{HashListing, PartFacts};
use mailrs_reputation::{Facts, Listing, Mark, Marks, Source};
use mailrs_screening_lua::{Action, DEFAULT_SOURCE, Input, Rules};

fn run(
    source: &str,
    outside: bool,
    parts: &[PartFacts],
    rep: &Facts,
) -> Vec<mailrs_screening_lua::Hit> {
    let mut rules = Rules::compile(source).unwrap();
    let e = rules.evaluate(&Input {
        outside,
        parts,
        reputation: rep,
    });
    assert!(e.errors.is_empty(), "{:?}", e.errors);
    e.hits
}

fn default(outside: bool, parts: &[PartFacts], rep: &Facts) -> Vec<mailrs_screening_lua::Hit> {
    run(DEFAULT_SOURCE, outside, parts, rep)
}

fn ids(hits: &[mailrs_screening_lua::Hit]) -> Vec<&str> {
    hits.iter().map(|h| h.rule.as_str()).collect()
}

fn part(name: &str) -> PartFacts {
    PartFacts {
        name: name.into(),
        sha256: "ab".repeat(32),
        ..PartFacts::default()
    }
}

fn listing(id: &str, source: Source, age_days: i64) -> Listing {
    Listing {
        subject: "203.0.113.9".into(),
        id: id.into(),
        source,
        age_days,
        reason: format!("why {id}"),
    }
}

fn marks(domain: &str, never: bool, n: usize, people: usize, age_days: i64) -> Marks {
    Marks {
        domain: domain.into(),
        never,
        marks: (0..n)
            .map(|i| Mark {
                id: format!("m{i}"),
                source: Source::UserJunk,
                actor: format!("u{}", i % people),
                age_days,
            })
            .collect(),
    }
}

#[test]
fn the_default_bundle_registers_every_rule_and_is_quiet_on_nothing() {
    let rules = Rules::compile(DEFAULT_SOURCE).unwrap();
    assert_eq!(rules.rule_count(), 24);
    assert!(rules.version().starts_with("lua:"));
    assert!(default(true, &[part("a.txt")], &Facts::default()).is_empty());
}

#[test]
fn an_executable_from_outside_is_junk_and_from_inside_is_not() {
    let p = PartFacts {
        executable_name: Some("exe".into()),
        ..part("invoice.exe")
    };
    let hits = default(true, std::slice::from_ref(&p), &Facts::default());
    assert_eq!(ids(&hits), ["exe-name"]);
    let h = &hits[0];
    assert_eq!(h.action, Action::Junk);
    assert_eq!(h.kind, "attachment");
    assert_eq!(h.detail, "invoice.exe: exe");
    assert!(h.inspired_by.contains("rspamd"));
    assert_eq!(
        h.evidence,
        [format!("part 0 invoice.exe sha256 {}", "ab".repeat(32))]
    );
    assert!(default(false, &[p], &Facts::default()).is_empty());
}

#[test]
fn a_pdf_script_counts_even_from_inside() {
    let p = PartFacts {
        pdf_script: Some("/JavaScript".into()),
        ..part("a.pdf")
    };
    assert_eq!(
        ids(&default(false, &[p], &Facts::default())),
        ["pdf-script"]
    );
}

#[test]
fn smuggling_needs_a_writer_and_a_long_run() {
    let short = PartFacts {
        html_writer: Some("msSaveOrOpenBlob".into()),
        html_base64_run: 19_999,
        ..part("a.html")
    };
    assert!(default(true, &[short], &Facts::default()).is_empty());
    let long = PartFacts {
        html_writer: Some("msSaveOrOpenBlob".into()),
        html_base64_run: 20_000,
        ..part("a.html")
    };
    assert_eq!(
        ids(&default(true, &[long], &Facts::default())),
        ["html-smuggling"]
    );
}

#[test]
fn a_listed_hash_cites_its_observation_and_age() {
    let p = PartFacts {
        hash_listed: Some(HashListing {
            id: "h1".into(),
            age_days: 4,
            reason: "seen in the 10-24 wave".into(),
        }),
        ..part("a.bin")
    };
    let hits = default(false, &[p], &Facts::default());
    assert_eq!(ids(&hits), ["own-hash-listed"]);
    assert_eq!(hits[0].evidence[1], "h1 (4 days): seen in the 10-24 wave");
}

#[test]
fn a_hand_checked_listing_is_junk_with_its_evidence() {
    let rep = Facts {
        ip: vec![listing("i1", Source::Incident, 400)],
        ..Facts::default()
    };
    let hits = default(true, &[], &rep);
    assert_eq!(ids(&hits), ["ip-incident-manual"]);
    assert_eq!(hits[0].evidence, ["i1 (incident, 400 days): why i1"]);
    assert_eq!(hits[0].detail, "203.0.113.9 is listed");
}

#[test]
fn abuse_counts_only_inside_its_lifetime() {
    let fresh = Facts {
        ip: vec![listing("a1", Source::Abuse, 90)],
        ..Facts::default()
    };
    let hits = default(true, &[], &fresh);
    assert_eq!(ids(&hits), ["ip-abuse"]);
    assert_eq!((hits[0].action, hits[0].score), (Action::Score, 3.0));
    let old = Facts {
        ip: vec![listing("a1", Source::Abuse, 91)],
        ..Facts::default()
    };
    assert!(default(true, &[], &old).is_empty());
}

#[test]
fn a_lifetime_is_data_a_bundle_can_change() {
    let longer = format!("{DEFAULT_SOURCE}\npolicy.lifetime_days.abuse = 365\n");
    let old = Facts {
        ip: vec![listing("a1", Source::Abuse, 200)],
        ..Facts::default()
    };
    assert_eq!(ids(&run(&longer, true, &[], &old)), ["ip-abuse"]);
}

#[test]
fn user_junk_needs_enough_people_and_marks_and_never_lists_a_never_domain() {
    let with = |m: Marks| Facts {
        sender_marks: Some(m),
        ..Facts::default()
    };
    assert_eq!(
        ids(&default(true, &[], &with(marks("ex.net", false, 5, 3, 2)))),
        ["sender-user-junk"]
    );
    assert!(
        default(true, &[], &with(marks("ex.net", false, 5, 2, 2))).is_empty(),
        "two people"
    );
    assert!(
        default(true, &[], &with(marks("ex.net", false, 4, 3, 2))).is_empty(),
        "four marks"
    );
    assert!(
        default(true, &[], &with(marks("ex.net", false, 5, 3, 31))).is_empty(),
        "outside the window"
    );
    assert!(
        default(true, &[], &with(marks("gmail.com", true, 50, 20, 1))).is_empty(),
        "never-list"
    );
}

#[test]
fn one_source_cannot_add_more_than_its_cap() {
    let rep = Facts {
        sender_marks: Some(marks("ex.net", false, 5, 3, 1)),
        link_marks: vec![marks("bad.example", false, 6, 4, 1)],
        ..Facts::default()
    };
    let hits = default(true, &[], &rep);
    assert_eq!(ids(&hits), ["sender-user-junk", "link-user-junk"]);
    assert_eq!((hits[0].score, hits[1].score), (2.5, 0.5));
}

#[test]
fn failing_authentication_adds_only_on_top_of_a_listing() {
    let failed = Facts {
        dmarc_fail: true,
        ..Facts::default()
    };
    assert!(default(true, &[], &failed).is_empty());
    let listed = Facts {
        dmarc_fail: true,
        ip: vec![listing("g1", Source::LoginGuard, 3)],
        ..Facts::default()
    };
    assert_eq!(
        ids(&default(true, &[], &listed)),
        ["ip-login-guard", "auth-fail-listed"]
    );
}

#[test]
fn a_bundle_is_checked_before_it_is_used() {
    let policy = include_str!("../rules/policy.lua");
    let good = "rule({id='x', kind='reputation', action='score', score=1, reason='r', inspired_by='', check=function(m) end})";
    assert!(Rules::compile(&format!("{policy}\n{good}")).is_ok());
    assert!(Rules::compile(good).is_err(), "no policy");
    assert!(
        Rules::compile(&format!("{policy}\n{good}\n{good}")).is_err(),
        "duplicate id"
    );
    let no_reason = good.replace("reason='r', ", "");
    assert!(Rules::compile(&format!("{policy}\n{no_reason}")).is_err());
    let bad_kind = good.replace("'reputation'", "'content'");
    assert!(Rules::compile(&format!("{policy}\n{bad_kind}")).is_err());
    let no_score = good.replace("score=1", "score=0");
    assert!(
        Rules::compile(&format!("{policy}\n{no_score}")).is_err(),
        "a score rule needs a score"
    );
}

#[test]
fn a_runaway_rule_is_stopped_and_reported() {
    let source = format!(
        "{}\nrule({{id='spin', kind='attachment', action='junk', score=0, reason='r', inspired_by='', check=function(m) while true do end end}})",
        include_str!("../rules/policy.lua")
    );
    let mut rules = Rules::compile(&source).unwrap();
    let rep = Facts::default();
    let e = rules.evaluate(&Input {
        outside: true,
        parts: &[],
        reputation: &rep,
    });
    assert!(e.hits.is_empty());
    assert_eq!(e.errors.len(), 1);
    assert!(e.errors[0].starts_with("rule 1:"), "{}", e.errors[0]);
}

#[test]
fn only_listed_versions_are_approved() {
    for line in mailrs_screening_lua::APPROVED
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let v = line.split_whitespace().next().unwrap();
        assert!(v.starts_with("lua:") && v.len() == 68, "{line}");
        assert_eq!(
            line.split_whitespace().count(),
            3,
            "version, report and date: {line}"
        );
        assert!(mailrs_screening_lua::approved(v));
        assert!(mailrs_screening_lua::approved(&format!(
            "{v};fallback-from=lua:00"
        )));
    }
    assert!(!mailrs_screening_lua::approved("lua:not-listed"));
    assert!(!mailrs_screening_lua::approved(""));
}
