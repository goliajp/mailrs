//! Each hold appends one fact, with what the reader had unread then.

use crate::hold_facts::{REPORT, holds};
use crate::ingest::ingest_delivered_file;
use crate::tests::fresh_state;

const USER: &str = "bob@golia.jp";

const PHISH: &[u8] = b"From: Bank <alerts@outside.example>\r\n\
To: bob@golia.jp\r\n\
Subject: Your account is locked\r\n\
Message-ID: <p1@outside.example>\r\n\
Date: Tue, 5 Aug 2026 09:00:00 +0900\r\n\
\r\n\
Confirm your password here.\r\n";

const FOLLOW_UP: &[u8] = b"From: Bank <alerts@outside.example>\r\n\
To: bob@golia.jp\r\n\
Subject: Re: Your account is locked\r\n\
Message-ID: <p2@outside.example>\r\n\
In-Reply-To: <p1@outside.example>\r\n\
References: <p1@outside.example>\r\n\
Date: Tue, 5 Aug 2026 10:00:00 +0900\r\n\
\r\n\
Last reminder.\r\n";

#[test]
fn a_hold_on_arrival_is_one_fact_and_holding_again_adds_none() {
    let state = fresh_state();
    let v = super::tests::held_verdict();
    ingest_delivered_file(&state, USER, "p1.eml", PHISH, "INBOX", Some(&v));
    ingest_delivered_file(&state, USER, "p2.eml", FOLLOW_UP, "INBOX", Some(&v));

    let facts = holds(&state, USER, "p1@outside.example").expect("facts");
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].source, "ingest");
    assert_eq!(facts[0].rules_version, "2026-11-05.1");
    assert_eq!(facts[0].unread_mids, ["p1@outside.example"]);
    assert!(facts[0].at > 0);
}

#[test]
fn a_rescan_hold_records_only_what_was_unread() {
    let state = fresh_state();
    let tid = "p1@outside.example";
    ingest_delivered_file(&state, USER, "p1.eml", PHISH, "INBOX", None);
    crate::routes::thread_actions::mark_thread_read_everywhere(&state, USER, tid);
    ingest_delivered_file(&state, USER, "p2.eml", FOLLOW_UP, "INBOX", None);
    let v: mailrs_inbound::FraudVerdict =
        serde_json::from_str(&super::tests::held_verdict()).expect("verdict");
    crate::maintenance::fraud_rescan::holding::apply(&state, USER, tid, tid, &v, "rescan")
        .expect("apply");

    let facts = holds(&state, USER, tid).expect("facts");
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].source, "rescan");
    assert_eq!(facts[0].unread_mids, ["p2@outside.example"]);
}

#[test]
fn a_report_hold_is_a_fact_and_a_delete_takes_the_facts() {
    let state = fresh_state();
    ingest_delivered_file(&state, USER, "p1.eml", PHISH, "INBOX", None);
    let tid = "p1@outside.example";
    let taken = crate::rejudge::recall::hold_reported(&state, USER, tid, None, "obs-1");
    assert_eq!(taken, Ok(true));

    let facts = holds(&state, USER, tid).expect("facts");
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].source, REPORT);
    assert_eq!(facts[0].rules_version, "");
    assert_eq!(facts[0].unread_mids, [tid]);

    crate::routes::thread_actions::delete_removing(&state, USER, tid, |_, _, _| Ok(None))
        .expect("delete");
    assert_eq!(holds(&state, USER, tid).expect("facts"), []);
}
