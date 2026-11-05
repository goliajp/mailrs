//! "Someone the reader wrote to" means the reader sent mail to them.
//!
//! Every delivery adds its sender to the reader's autocomplete store, so
//! that store holds whoever wrote in. The correspondent facts the rules get
//! must come from what the reader sent, on the receive path and on a
//! re-judgement alike.

use kevy_client::Connection;
use mailrs_core_sidestate::families::{contact_cards, contact_sent, contacts};
use mailrs_fraud::relation::Relation;
use mailrs_fraud_lua::stored::relation::{Counted, relation};

const ME: &str = "me@golia.example";

/// What delivery leaves behind for a sender: an autocomplete entry and a
/// received count.
fn delivered_from(c: &mut Connection, shown: &str, address: &str, received: u64) {
    c.hset(
        contact_cards::autocomplete_key(ME).as_bytes(),
        &[(address.as_bytes(), shown.as_bytes())],
    )
    .unwrap();
    let sent = contacts::scoring_for(c, ME, address).sent_count;
    contact_sent::set_counts(c, ME, address, received, u64::from(sent)).unwrap();
}

fn wrote_to(c: &mut Connection, address: &str, sent: u64) {
    let received = contacts::scoring_for(c, ME, address).received_count;
    contact_sent::set_counts(c, ME, address, u64::from(received), sent).unwrap();
}

fn store(name: &str) -> Connection {
    let mut c = Connection::connect(&format!("mem://{name}")).unwrap();
    // a stranger who only ever wrote in, and a partner the reader wrote to
    delivered_from(
        &mut c,
        "Stranger <billing@stranger.example>",
        "billing@stranger.example",
        5,
    );
    delivered_from(
        &mut c,
        "田中 太郎 <tanaka@partner.example>",
        "tanaka@partner.example",
        2,
    );
    wrote_to(&mut c, "tanaka@partner.example", 3);
    c
}

#[test]
fn a_sender_who_only_wrote_in_is_not_a_correspondent() {
    let mut c = store("corr-inbound-only");
    let r = relation(
        Some(&mut c),
        ME,
        "Stranger <billing@stranger.example>",
        "",
        Counted::Before,
    );
    assert!(!r.from_correspondent);
    assert!(!r.from_correspondent_domain);
    // another address at the stranger's domain is not one either
    let r = relation(
        Some(&mut c),
        ME,
        "ops@stranger.example",
        "",
        Counted::Before,
    );
    assert!(!r.from_correspondent_domain);
    // nor is the stranger's name borrowed by anyone
    let r = relation(
        Some(&mut c),
        ME,
        "Stranger <stranger.billing@free.example>",
        "",
        Counted::Before,
    );
    assert!(!r.name_borrowed);
    // and a domain one letter from the stranger's is no look-alike
    let r = relation(Some(&mut c), ME, "x@stranqer.example", "", Counted::Before);
    assert!(!r.looks_like_correspondent);
}

#[test]
fn a_sender_the_reader_wrote_to_is_a_correspondent() {
    let mut c = store("corr-written-to");
    let r = relation(
        Some(&mut c),
        ME,
        "田中 太郎 <tanaka@partner.example>",
        "",
        Counted::Before,
    );
    assert!(r.from_correspondent && r.from_correspondent_domain);
    let r = relation(
        Some(&mut c),
        ME,
        "sato@partner.example",
        "",
        Counted::Before,
    );
    assert!(!r.from_correspondent && r.from_correspondent_domain);
    let r = relation(
        Some(&mut c),
        ME,
        "田中 太郎 <tanaka.partner@free.example>",
        "",
        Counted::Before,
    );
    assert!(r.name_borrowed);
}

/// The receive path reads before the arrival is counted, a re-judgement
/// after; both must hand the rules the same relation.
#[test]
fn the_receive_path_and_a_rejudgement_agree() {
    for (i, from) in [
        "Stranger <billing@stranger.example>",
        "田中 太郎 <tanaka@partner.example>",
        "sato@partner.example",
        "田中 太郎 <tanaka.partner@free.example>",
    ]
    .into_iter()
    .enumerate()
    {
        let mut c = store(&format!("corr-agree-{i}"));
        let live = relation(Some(&mut c), ME, from, "", Counted::Before);

        let address = mailrs_fraud::impersonation::address_of(from)
            .unwrap()
            .trim_end_matches('>')
            .to_ascii_lowercase();
        let before = contacts::scoring_for(&mut c, ME, &address);
        contact_sent::set_counts(
            &mut c,
            ME,
            &address,
            u64::from(before.received_count) + 1,
            u64::from(before.sent_count),
        )
        .unwrap();

        let raw = format!("From: {from}\r\nTo: {ME}\r\nSubject: hi\r\n\r\nbody\r\n");
        let mut seen: Option<Relation> = None;
        let mut hist = Some(c);
        mailrs_fraud_lua::stored::judge_with(
            raw.as_bytes(),
            Some((ME, Counted::After)),
            None,
            &[],
            &mailrs_fraud::Policy::default(),
            &mut hist,
            &mut |facts, _| {
                seen = Some(facts.relation.clone());
                Ok(mailrs_fraud_lua::Scan {
                    findings: Default::default(),
                    version: String::new(),
                    shadow: Vec::new(),
                })
            },
        )
        .unwrap();
        assert_eq!(seen.as_ref(), Some(&live), "{from}");
    }
}

/// The `Reply-To` half: whether the reader has written to the address
/// replies go to, whoever the `From:` is.
#[test]
fn a_reply_address_the_reader_wrote_to_is_a_correspondent() {
    let mut c = store("corr-reply-to");
    let helpdesk = "Support <noreply@helpdesk-mail.com>";
    let r = relation(
        Some(&mut c),
        ME,
        helpdesk,
        "Tanaka@Partner.example",
        Counted::Before,
    );
    assert!(r.reply_to_correspondent && !r.from_correspondent);
    let r = relation(
        Some(&mut c),
        ME,
        helpdesk,
        "billing@stranger.example",
        Counted::Before,
    );
    assert!(!r.reply_to_correspondent, "only wrote in");
    let r = relation(Some(&mut c), ME, helpdesk, "", Counted::Before);
    assert!(!r.reply_to_correspondent, "no Reply-To");
}
