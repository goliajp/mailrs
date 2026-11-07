//! The shipped "claims to be a reply, belongs to no thread" rules, judged
//! on whole messages with the reader's history: a helpdesk whose replies
//! go to a vendor the reader writes to is scored, not held; the same shape
//! from nobody the reader knows is held.
use kevy_client::Connection;
use mailrs_core_sidestate::families::contact_sent;
use mailrs_fraud_lua::stored::relation::Counted;
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const ME: &str = "me@golia.example";

fn helpdesk(subject: &str, thread: &str) -> Vec<u8> {
    format!(
        "From: Acme Support <noreply@acme-tickets.com>\r\n\
Reply-To: support@acmesoft.com\r\n\
To: {ME}\r\nSubject: {subject}\r\n{thread}\
Date: Tue, 15 Sep 2026 09:00:00 +0000\r\n\r\n\
Your request has been updated.\r\n"
    )
    .into_bytes()
}

/// `(held, the rules that fired)` for `raw`, read by `ME` against `store`.
fn judge(raw: &[u8], store: Connection) -> (bool, Vec<String>) {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let mut hist = Some(store);
    let j = mailrs_fraud_lua::stored::judge_with(
        raw,
        Some((ME, Counted::After)),
        None,
        &[],
        &mailrs_fraud::Policy::default(),
        &mut hist,
        &mut |facts, policy| {
            let out = rules.classify(facts, policy);
            assert!(out.errors.is_empty(), "{:?}", out.errors);
            Ok(mailrs_fraud_lua::Scan {
                findings: out.findings,
                version: String::new(),
                shadow: Vec::new(),
            })
        },
    )
    .unwrap();
    let mut fired: Vec<String> = j.findings.rules().iter().map(|r| r.to_string()).collect();
    fired.sort();
    (j.holds, fired)
}

fn reader_wrote_to(name: &str, address: &str) -> Connection {
    let mut c = Connection::connect(&format!("mem://{name}")).unwrap();
    contact_sent::set_counts(&mut c, ME, address, 0, 2).unwrap();
    c
}

fn nobody(name: &str) -> Connection {
    Connection::connect(&format!("mem://{name}")).unwrap()
}

#[test]
fn a_helpdesk_reply_to_someone_the_reader_wrote_to_is_scored_not_held() {
    let raw = helpdesk("Re: [#4821] license renewal", "");
    let (held, fired) = judge(&raw, reader_wrote_to("a-reply-to", "support@acmesoft.com"));
    assert!(!held, "{fired:?}");
    assert_eq!(fired, ["reply-claim-without-thread-correspondent"]);
    // the From side counts the same
    let (held, fired) = judge(&raw, reader_wrote_to("a-from", "noreply@acme-tickets.com"));
    assert!(!held, "{fired:?}");
    assert_eq!(fired, ["reply-claim-without-thread-correspondent"]);
}

#[test]
fn the_same_shape_from_nobody_the_reader_knows_is_held() {
    let raw = helpdesk("Re: [#4821] license renewal", "");
    let (held, fired) = judge(&raw, nobody("a-nobody"));
    assert!(held, "{fired:?}");
    assert_eq!(fired, ["reply-claim-without-thread"]);
    // a full-width colon, as Japanese input writes it
    let (held, _) = judge(&helpdesk("Re：お見積りの件", ""), nobody("a-fullwidth"));
    assert!(held);
    // the reader wrote to someone else entirely: still nobody this message names
    let (held, _) = judge(&raw, reader_wrote_to("a-other", "tanaka@partner.com"));
    assert!(held);
}

#[test]
fn a_reply_that_names_its_thread_fires_neither() {
    let raw = helpdesk(
        "Re: [#4821] license renewal",
        "In-Reply-To: <4821@acmesoft.com>\r\n",
    );
    let (held, fired) = judge(&raw, nobody("a-threaded"));
    assert!(!held);
    assert!(fired.is_empty(), "{fired:?}");
}
