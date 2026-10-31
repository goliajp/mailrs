use mailrs_reputation::{Auth, Kind, Message, NeverList, Observation, Set, Source, Stance};

const NOW: i64 = 1_800_000_000;
const DAY: i64 = 86_400;

fn obs(
    id: &str,
    days_ago: i64,
    source: Source,
    kind: Kind,
    value: &str,
    stance: Stance,
) -> Observation {
    Observation {
        id: id.into(),
        at: NOW - days_ago * DAY,
        source,
        kind,
        value: value.into(),
        stance,
        retracts: None,
        reason: format!("reason {id}"),
        actor: None,
    }
}

fn junk(id: &str, days_ago: i64, kind: Kind, value: &str, user: &str) -> Observation {
    Observation {
        actor: Some(user.into()),
        ..obs(id, days_ago, Source::UserJunk, kind, value, Stance::Deny)
    }
}

fn message() -> Message {
    Message {
        client_ip: Some("203.0.113.9".parse().unwrap()),
        envelope_domain: "bounce.mail.example.net".into(),
        header_domain: "Mail.Example.NET".into(),
        link_hosts: vec!["x.bad.example.org".into(), "cdn.bad.example.org".into()],
        auth: Auth {
            dmarc_fail: true,
            spf_fail: false,
        },
    }
}

fn never() -> NeverList {
    NeverList::builtin(&[])
}

#[test]
fn listings_carry_their_observation_and_age() {
    let set = Set::build(
        &[
            obs(
                "a",
                3,
                Source::Incident,
                Kind::Ip,
                "203.0.113.9",
                Stance::Deny,
            ),
            obs(
                "b",
                400,
                Source::Manual,
                Kind::Cidr,
                "203.0.113.0/24",
                Stance::Deny,
            ),
            obs(
                "c",
                1,
                Source::Abuse,
                Kind::SenderDomain,
                "example.net",
                Stance::Deny,
            ),
            obs(
                "d",
                0,
                Source::Incident,
                Kind::LinkDomain,
                "bad.example.org",
                Stance::Deny,
            ),
        ],
        &never(),
        NOW,
    );
    let f = set.facts(&message());
    let ids = |l: &[mailrs_reputation::Listing]| l.iter().map(|x| x.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&f.ip), ["a", "b"]);
    assert_eq!(
        f.ip[1].age_days, 400,
        "old listings are kept; the rules judge age"
    );
    assert_eq!(f.ip[0].source, Source::Incident);
    assert_eq!(f.ip[0].reason, "reason a");
    assert_eq!(ids(&f.envelope), ["c"]);
    assert_eq!(ids(&f.header), ["c"]);
    assert_eq!(ids(&f.links), ["d", "d"], "one per linked host");
    assert!(f.dmarc_fail && !f.spf_fail);
}

#[test]
fn nothing_listed_means_empty_facts() {
    let set = Set::build(&[], &never(), NOW);
    let f = set.facts(&message());
    assert!(f.ip.is_empty() && f.envelope.is_empty() && f.header.is_empty() && f.links.is_empty());
    assert!(f.sender_marks.is_none() && f.link_marks.is_empty());
}

#[test]
fn an_allow_hides_every_deny_of_its_subject() {
    let set = Set::build(
        &[
            obs(
                "a",
                3,
                Source::Incident,
                Kind::Ip,
                "203.0.113.9",
                Stance::Deny,
            ),
            obs(
                "b",
                2,
                Source::Manual,
                Kind::Cidr,
                "203.0.113.0/28",
                Stance::Allow,
            ),
            obs(
                "c",
                1,
                Source::Abuse,
                Kind::SenderDomain,
                "example.net",
                Stance::Deny,
            ),
            obs(
                "d",
                0,
                Source::Manual,
                Kind::SenderDomain,
                "example.net",
                Stance::Allow,
            ),
            junk("e", 0, Kind::SenderDomain, "example.net", "u1"),
        ],
        &never(),
        NOW,
    );
    let f = set.facts(&message());
    assert!(f.ip.is_empty());
    assert!(f.envelope.is_empty() && f.header.is_empty());
    assert!(f.sender_marks.is_none());
}

#[test]
fn a_retraction_removes_what_it_names() {
    let mut r = obs(
        "r",
        0,
        Source::Manual,
        Kind::Ip,
        "203.0.113.9",
        Stance::Retract,
    );
    r.retracts = Some("a".into());
    let set = Set::build(
        &[
            obs(
                "a",
                3,
                Source::Incident,
                Kind::Ip,
                "203.0.113.9",
                Stance::Deny,
            ),
            r,
        ],
        &never(),
        NOW,
    );
    assert!(set.facts(&message()).ip.is_empty());
    assert_eq!(set.observations(), 0);
}

#[test]
fn marks_group_by_registrable_domain_newest_first() {
    let set = Set::build(
        &[
            junk("old", 10, Kind::SenderDomain, "news.example.net", "u1"),
            junk("new", 1, Kind::SenderDomain, "example.net", "u2"),
            junk("l1", 2, Kind::LinkDomain, "a.bad.example.org", "u1"),
            Observation {
                source: Source::Complaint,
                ..junk("l2", 1, Kind::LinkDomain, "bad.example.org", "fbl")
            },
        ],
        &never(),
        NOW,
    );
    let f = set.facts(&message());
    let s = f.sender_marks.expect("marks on example.net");
    assert_eq!(s.domain, "example.net");
    assert!(!s.never);
    let ids: Vec<_> = s
        .marks
        .iter()
        .map(|m| (m.id.as_str(), m.actor.as_str(), m.age_days))
        .collect();
    assert_eq!(ids, [("new", "u2", 1), ("old", "u1", 10)]);
    assert_eq!(
        f.link_marks.len(),
        1,
        "two hosts of one domain give one entry"
    );
    assert_eq!(f.link_marks[0].domain, "example.org");
    assert_eq!(f.link_marks[0].marks.len(), 2);
    assert!(
        f.ip.is_empty() && f.envelope.is_empty(),
        "marks are not listings"
    );
}

#[test]
fn never_listed_domains_are_flagged() {
    let set = Set::build(
        &[junk("g", 0, Kind::SenderDomain, "gmail.com", "u1")],
        &never(),
        NOW,
    );
    let m = Message {
        header_domain: "gmail.com".into(),
        ..message()
    };
    assert!(set.facts(&m).sender_marks.unwrap().never);
    let own = NeverList::builtin(&["example.net".to_string()]);
    let set = Set::build(
        &[junk("o", 0, Kind::SenderDomain, "example.net", "u1")],
        &own,
        NOW,
    );
    assert!(set.facts(&message()).sender_marks.unwrap().never);
}

#[test]
fn marks_are_capped() {
    let many: Vec<Observation> = (0..600)
        .map(|i| {
            junk(
                &format!("m{i}"),
                i % 30,
                Kind::SenderDomain,
                "example.net",
                &format!("u{i}"),
            )
        })
        .collect();
    let set = Set::build(&many, &never(), NOW);
    let marks = set.facts(&message()).sender_marks.unwrap().marks;
    assert_eq!(marks.len(), 500);
    assert!(marks.iter().all(|m| m.age_days <= marks[499].age_days));
}
