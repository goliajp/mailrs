//! Narrowing the one list to some of the connected accounts.
//!
//! The lists are unified on purpose: Inbox holds everything, from this
//! deployment's own address and from every mailbox connected to it.
//! The filter is "only these", not "only this" — somebody with four
//! accounts wants work and personal together and the two others out.

use std::sync::Arc;

use kevy_embedded::{Config, Store};

use crate::KevyMailboxStore;
use crate::list_threads::*;

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    s.ensure_thread_table();
    s
}

fn row(tid: &str, date: i64, account: &str) -> ThreadRow {
    ThreadRow {
        account_id: account.into(),
        thread_id: tid.into(),
        subject: format!("subject of {tid}"),
        senders_csv: "x@y.z".into(),
        count: 1,
        unread_count: 0,
        latest_date: date,
        latest_preview: String::new(),
        category: "inbox".into(),
        importance_level: "normal".into(),
        importance_score: 0.0,
        requires_action: false,
        pinned: false,
        archived: false,
        has_action: false,
        sent_count: 0,
        starred: false,
        snoozed_until: 0,
    }
}

fn seeded() -> KevyMailboxStore {
    let s = store();
    let u = "me@golia.jp";
    s.upsert_thread(u, &row("own", 300, "")).unwrap();
    s.upsert_thread(u, &row("gmail", 200, "ext_gmail")).unwrap();
    s.upsert_thread(u, &row("qq", 100, "ext_qq")).unwrap();
    s
}

fn ids(s: &KevyMailboxStore, accounts: Option<Vec<String>>) -> Vec<String> {
    let f = ListThreadsFilter {
        accounts,
        ..ListThreadsFilter::default()
    };
    s.list_threads_by_activity("me@golia.jp", &f, 0, 50)
        .expect("list")
        .0
        .into_iter()
        .map(|r| r.thread_id)
        .collect()
}

/// No filter is everything, which is what a unified inbox means.
#[test]
fn without_a_filter_every_account_is_in_the_list() {
    let s = seeded();
    assert_eq!(ids(&s, None), vec!["own", "gmail", "qq"]);
}

#[test]
fn one_account_narrows_to_it() {
    let s = seeded();
    assert_eq!(ids(&s, Some(vec!["ext_gmail".into()])), vec!["gmail"]);
}

/// "Only these", not "only this".
#[test]
fn several_accounts_narrow_to_those() {
    let s = seeded();
    let got = ids(&s, Some(vec!["ext_gmail".into(), "ext_qq".into()]));
    assert_eq!(got, vec!["gmail", "qq"]);
}

/// This deployment's own mail is an account in the filter like any
/// other, or it cannot be switched off — and a person who connected
/// Gmail and wants only Gmail is asking exactly that.
#[test]
fn this_servers_own_mail_can_be_asked_for_and_left_out() {
    let s = seeded();
    assert_eq!(ids(&s, Some(vec![String::new()])), vec!["own"]);
    assert!(!ids(&s, Some(vec!["ext_qq".into()])).contains(&"own".to_string()));
}

/// Order is still recency. A filter narrows the list; it does not
/// reorder it, and a filtered Inbox that is not newest-first reads as
/// broken.
#[test]
fn filtering_does_not_disturb_the_order() {
    let s = seeded();
    let got = ids(&s, Some(vec!["ext_qq".into(), "ext_gmail".into()]));
    assert_eq!(got, vec!["gmail", "qq"], "the newer one came second");
}

/// An empty list of accounts is a filter that nothing satisfies, and
/// the honest answer is nothing. Treating it as "no filter" would show
/// everything to somebody who had just unchecked every box.
#[test]
fn unchecking_everything_shows_nothing() {
    let s = seeded();
    assert!(ids(&s, Some(Vec::new())).is_empty());
}

/// A row written before connected mailboxes existed has no account at
/// all, and it is this deployment's own — not a fourth account nobody
/// can see in the filter.
#[test]
fn a_row_from_before_this_existed_counts_as_our_own() {
    let s = store();
    let u = "me@golia.jp";
    let mut r = row("legacy", 400, "");
    r.account_id = String::new();
    s.upsert_thread(u, &r).unwrap();
    assert_eq!(ids(&s, Some(vec![String::new()])), vec!["legacy"]);
}
