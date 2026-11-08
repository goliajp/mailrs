//! The sectioned inbox: each core files the seeded mail into the same
//! sections, and accepting a first contact moves it out of New on both.
//! Both cores are also held to the shared fixtures, delivered message by
//! message to a reader of their own.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::senders::AcceptSendersRequest;
use mailrs_core_api::method::thread::SectionFactsWire;
use mailrs_core_api::types::ConversationFilter;

use super::ALICE;
use super::harness::{Cores, Diffs};

const MINTED: &[&str] = &["id", "mailbox_id", "modseq", "new_modseq", "affected"];

/// The sectioned inbox's views, compared with the rest after every step.
pub(super) fn views(base: &ConversationFilter) -> Vec<(&'static str, ConversationFilter)> {
    vec![
        (
            "section reply",
            ConversationFilter {
                section: Some("reply".into()),
                ..base.clone()
            },
        ),
        (
            "section awaiting",
            ConversationFilter {
                section: Some("awaiting".into()),
                ..base.clone()
            },
        ),
        (
            "section awaiting, due",
            ConversationFilter {
                section: Some("awaiting".into()),
                followup_due_by: Some(1_700_000_650),
                ..base.clone()
            },
        ),
        (
            "section new",
            ConversationFilter {
                section: Some("new".into()),
                ..base.clone()
            },
        ),
        (
            "section feed",
            ConversationFilter {
                section: Some("feed".into()),
                ..base.clone()
            },
        ),
        (
            "section other",
            ConversationFilter {
                section: Some("other".into()),
                ..base.clone()
            },
        ),
    ]
}

fn section(name: &str) -> ListConversationsRequest {
    ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            section: Some(name.into()),
            ..Default::default()
        },
    }
}

async fn compare(c: &Cores, d: &mut Diffs, step: &str) -> Vec<(String, Vec<String>)> {
    let mut seen = Vec::new();
    for name in ["reply", "awaiting", "new", "feed", "other"] {
        let rows = d.both(
            &format!("section {name} {step}"),
            c.fastcore.list_conversations(ALICE, &section(name)).await,
            c.pg_core.list_conversations(ALICE, &section(name)).await,
            MINTED,
        );
        let ids = rows
            .map(|(r, _)| {
                r.items
                    .into_iter()
                    .map(|i| i.thread_id.to_string())
                    .collect()
            })
            .unwrap_or_default();
        seen.push((name.to_string(), ids));
    }
    seen
}

fn has(seen: &[(String, Vec<String>)], section: &str, tid: &str) -> bool {
    seen.iter()
        .any(|(name, ids)| name == section && ids.iter().any(|i| i == tid))
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let before = compare(c, d, "before accepting").await;
    assert!(has(&before, "new", "t-intro@far.test"), "{before:?}");
    assert!(has(&before, "feed", "t-sale@shop.test"), "{before:?}");
    assert!(
        !before
            .iter()
            .any(|(_, ids)| ids.iter().any(|i| i == "t-spam@spam.test")),
        "Junk is in no section: {before:?}"
    );

    let req = AcceptSendersRequest {
        addresses: vec!["EVE@far.test".into(), "eve@far.test".into()],
        accepted_at: 1_700_001_000,
    };
    d.both(
        "accept_senders",
        c.fastcore.accept_senders(ALICE, &req).await,
        c.pg_core.accept_senders(ALICE, &req).await,
        &[],
    );
    let after = compare(c, d, "after accepting").await;
    assert!(!has(&after, "new", "t-intro@far.test"), "{after:?}");
    // accepted, but nothing has gone back to them: not owed a reply
    assert!(has(&after, "other", "t-intro@far.test"), "{after:?}");
}

/// A reader of their own, so the fixtures' conversations stay out of every
/// other scenario's mailbox.
const READER: &str = "fixtures@example.test";

fn wire(f: mailrs_mailbox::section::MessageFacts) -> SectionFactsWire {
    SectionFactsWire {
        direct: f.direct,
        from_person: f.from_person,
        first_contact: f.first_contact,
        to_others: f.to_others,
        invite: f.invite,
        mutual: f.mutual,
    }
}

/// The messages that give a fixture's `Latest`: the reader's own, an
/// earlier one from a person when the last is not theirs, and the last one
/// from someone else. `(at, sender, facts)`.
fn messages(
    l: &mailrs_mailbox::section::Latest,
    sender: &str,
) -> Vec<(i64, String, SectionFactsWire)> {
    let mut out = Vec::new();
    if l.own_at > 0 {
        let mine = SectionFactsWire {
            to_others: l.own_to_others,
            ..SectionFactsWire::default()
        };
        out.push((l.own_at, READER.to_string(), mine));
    }
    if l.person_at > 0 && l.person_at != l.other_at {
        let person = SectionFactsWire {
            direct: true,
            from_person: true,
            ..SectionFactsWire::default()
        };
        out.push((l.person_at, sender.to_string(), person));
    }
    if l.other_at > 0 {
        out.push((l.other_at, sender.to_string(), wire(l.other)));
    }
    out
}

/// Every fixture `section_of` is held to, delivered message by message to
/// both cores: each conversation must land in the fixture's section on
/// both, which holds the SQL core's `CASE` to the same answers.
pub(super) async fn fixtures(c: &Cores, d: &mut Diffs) {
    let mut uid = 0;
    let mut want = Vec::new();
    for (i, (name, latest, expected)) in mailrs_mailbox::section::fixtures().into_iter().enumerate()
    {
        let tid = format!("t-fixture-{i}@fixtures.test");
        let sender = format!("s{i}@fixtures.test");
        for (j, (at, from, facts)) in messages(&latest, &sender).into_iter().enumerate() {
            uid += 1;
            let mid = format!("m{i}-{j}@fixtures.test");
            let arrival = super::mail::Arrival {
                message_id: &mid,
                uid,
                thread_id: &tid,
                user: READER,
                sender: &from,
                subject: name,
                date: at,
                category: "inbox",
                unread: from != READER,
            };
            let mut req = arrival.request();
            req.section_facts = Some(facts);
            d.both(
                &format!("deliver fixture {name}"),
                c.fastcore.deliver_message(READER, &tid, &req).await,
                c.pg_core.deliver_message(READER, &tid, &req).await,
                MINTED,
            );
        }
        if latest.other_accepted {
            let req = AcceptSendersRequest {
                addresses: vec![sender.clone()],
                accepted_at: 1_700_002_000,
            };
            d.both(
                &format!("accept fixture {name}"),
                c.fastcore.accept_senders(READER, &req).await,
                c.pg_core.accept_senders(READER, &req).await,
                &[],
            );
        }
        want.push((name, tid, expected));
    }
    for (core, client) in [("fastcore", &c.fastcore), ("pg-core", &c.pg_core)] {
        let mut seen = Vec::new();
        for s in mailrs_mailbox::section::Section::ALL {
            let rows = client.list_conversations(READER, &section(s.name())).await;
            let ids: Vec<String> = rows
                .map(|r| {
                    r.items
                        .into_iter()
                        .map(|i| i.thread_id.to_string())
                        .collect()
                })
                .unwrap_or_default();
            seen.push((s.name().to_string(), ids));
        }
        for (name, tid, expected) in &want {
            if !has(&seen, expected.name(), tid) {
                d.0.push(format!(
                    "{core}: fixture {name:?} is not in {}: {seen:?}",
                    expected.name()
                ));
            }
        }
    }
}
