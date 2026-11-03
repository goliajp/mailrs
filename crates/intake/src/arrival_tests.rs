//! The threading rule against a thread index held in memory.

use std::collections::HashMap;

use crate::arrival::{Threads, thread_root};

/// One conversation, `q1@x` under "Quarterly report".
struct One(HashMap<&'static str, &'static str>);

impl Threads for One {
    fn thread_for_message_id(&self, _user: &str, mid: &str) -> Option<String> {
        self.0.get(mid).map(|t| t.to_string())
    }
    fn thread_subject(&self, _user: &str, tid: &str) -> Option<String> {
        (tid == "q1@x").then(|| "Quarterly report".to_string())
    }
}

fn root(irt: &str, refs: &[&str], subject: &str) -> String {
    let threads = One(HashMap::from([("q1@x", "q1@x")]));
    let refs: Vec<String> = refs.iter().map(|r| r.to_string()).collect();
    thread_root(&threads, "u@y", "new@x", irt, &refs, subject)
}

#[test]
fn a_reply_on_the_same_subject_joins_its_ancestor() {
    assert_eq!(root("q1@x", &[], "Re: Quarterly report"), "q1@x");
    assert_eq!(
        root("q9@x", &["q1@x", "q9@x"], "RE: quarterly report"),
        "q1@x"
    );
}

/// The root message's id is what In-Reply-To cites, so falling back to it
/// after the subject rule turned the ancestor down put the new topic
/// straight back into the old conversation.
#[test]
fn a_reply_that_changes_the_subject_starts_its_own_conversation() {
    assert_eq!(root("q1@x", &[], "Annual closing"), "new@x");
    assert_eq!(root("", &["q1@x"], "Annual closing"), "new@x");
}

#[test]
fn with_no_known_ancestor_the_headers_name_the_root() {
    assert_eq!(root("q7@x", &["q5@x", "q7@x"], "anything"), "q5@x");
    assert_eq!(root("q7@x", &[], "anything"), "q7@x");
    assert_eq!(root("", &[], "anything"), "new@x");
}

/// The reader's rule for a sender files the mail ahead of the classifier,
/// but never pulls mail out of Junk.
#[test]
fn a_sender_rule_files_the_mail_ahead_of_the_classifier() {
    use mailrs_core_sidestate::families::sender_rules::{SenderRule, put};
    let mut c = kevy_client::Connection::connect("mem://intake-sender-rule").unwrap();
    let rule = SenderRule {
        bucket: Some("promotions".into()),
        ..Default::default()
    };
    put(&mut c, "me@x.test", "news@shop.example", &rule).unwrap();
    let from = "Shop <News@Shop.example>";
    assert_eq!(
        crate::arrival::sender_rule_category(Some(&mut c), "me@x.test", from, "INBOX"),
        Some("promotion")
    );
    assert_eq!(
        crate::arrival::sender_rule_category(Some(&mut c), "me@x.test", from, "Junk"),
        None
    );
    assert_eq!(
        crate::arrival::sender_rule_category(Some(&mut c), "me@x.test", "other@x.example", "INBOX"),
        None
    );
}

fn placement_of(net: kevy_client::Connection, from: &str, body: &str, target: &str) -> Vec<String> {
    let threads = One(HashMap::new());
    let raw = format!(
        "From: {from}\r\nTo: me@x.test\r\nSubject: hello\r\nMessage-ID: <p1@x>\r\n\r\n{body}\r\n"
    );
    let a = crate::arrival::derive(
        &threads,
        Some(net),
        "me@x.test",
        "f1",
        raw.as_bytes(),
        target,
        None,
    );
    a.placement.iter().map(|r| r.code.clone()).collect()
}

/// Each way an arrival is filed out of the plain inbox names who did it:
/// the reader's sender rule, the classifier, or the reader's filter.
#[test]
fn every_filing_says_who_filed_it() {
    use mailrs_core_sidestate::families::sender_rules::{SenderRule, put};
    let mut c = kevy_client::Connection::connect("mem://intake-placement-rule").unwrap();
    let rule = SenderRule {
        bucket: Some("promotions".into()),
        ..Default::default()
    };
    put(&mut c, "me@x.test", "news@shop.example", &rule).unwrap();
    assert_eq!(
        placement_of(c, "news@shop.example", "sale", "INBOX"),
        ["sorted-by-your-rule"]
    );

    // the network test double does not pipeline, so the classifier is
    // never warm here; its half of the decision is checked directly
    let codes =
        |v: Vec<mailrs_inbound::Reason>| -> Vec<String> { v.into_iter().map(|r| r.code).collect() };
    assert_eq!(
        codes(crate::arrival::placement(false, "promotion", "INBOX")),
        ["sorted-by-classifier"]
    );
    assert_eq!(
        codes(crate::arrival::placement(false, "notification", "INBOX")),
        ["sorted-by-classifier"]
    );
    assert!(crate::arrival::placement(false, "inbox", "INBOX").is_empty());
    // a rule that keeps a sender in the inbox leaves it in the plain inbox
    assert!(crate::arrival::placement(true, "inbox", "INBOX").is_empty());

    let c = kevy_client::Connection::connect("mem://intake-placement-filter").unwrap();
    assert_eq!(
        placement_of(c, "a@else.example", "hi", "Junk"),
        ["junk-your-filter"]
    );
}

/// Mail the classifier leaves in the inbox, with no rule and no filter,
/// was not judged and carries nothing.
#[test]
fn plain_inbox_mail_carries_no_placement() {
    let c = kevy_client::Connection::connect("mem://intake-placement-none").unwrap();
    assert!(placement_of(c, "a@else.example", "hi", "INBOX").is_empty());
}
