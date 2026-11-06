//! The `mark-all-read` query string, checked against the same fixture as
//! the web client's `wireMarkAllRead`; `request_contract_registry.rs` lists the
//! fixture in `every_fixture_has_a_test`.
//!
//! Every list axis is in it: the route marks the list those parameters
//! name, read by the struct the list itself is read by, and an axis it
//! dropped would mark the folder or the mailbox around that list.

mod common;
use axum::extract::Query;
use axum::http::Uri;
use mailrs_webapi::handlers::conversation_verbs::MarkAllReadQuery;
use mailrs_webapi::handlers::conversations::ListQuery;

fn uri(query: &str) -> Uri {
    format!("/api/conversations/mark-all-read?{query}")
        .parse()
        .expect("uri")
}

fn read(query: &str) -> (MarkAllReadQuery, ListQuery) {
    let uri = uri(query);
    let Query(review) = Query::<MarkAllReadQuery>::try_from_uri(&uri).expect("review flag");
    let Query(list) = Query::<ListQuery>::try_from_uri(&uri).expect("list params");
    (review, list)
}

#[test]
fn mark_all_read_query_matches() {
    let params: serde_json::Map<String, serde_json::Value> =
        common::parse("conversation-mark-all-read");
    let pairs: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{k}={}", v.as_str().expect("a string")))
        .collect();
    let (review, list) = read(&pairs.join("&"));
    assert!(!review.quarantined);
    assert!(list.names_a_list());
    let f = list.filter();
    assert!(f.trashed && f.archived && f.include_quarantined && f.assigned_to_me);
    assert_eq!(f.label.as_deref(), Some("3"));
    assert_eq!(f.category.as_deref(), Some("notification"));
    assert_eq!(f.folder.as_deref(), Some("Inbox"));
    assert_eq!((f.unread, f.starred), (Some(true), Some(true)));
    assert_eq!((f.pinned, f.snoozed), (Some(true), Some(true)));
    assert_eq!(f.section.as_deref(), Some("awaiting"));
    assert_eq!(f.followup_due_by, Some(1_700_000_650));
    assert_eq!(f.accounts, Some(vec![String::new(), "ext_a".to_string()]));
}

/// Each list a client draws names itself, so none of them falls through
/// to the whole mailbox.
#[test]
fn every_list_names_itself() {
    for query in [
        "folder=Inbox",
        "archived=true",
        "trashed=true",
        "label=3",
        "folder=NonJunk&unread=true",
        "pinned=true",
        "snoozed=true",
        "section=awaiting",
        "assigned_to_me=true",
    ] {
        assert!(read(query).1.names_a_list(), "{query}");
    }
}

/// What iOS and Android sent before the route scoped beyond four axes,
/// and the web with none: no list named is still the whole mailbox.
#[test]
fn an_old_client_marks_what_it_marked_before() {
    let (review, list) = read("archived=false");
    assert!(!review.quarantined && !list.names_a_list());
    let (_, list) = read("");
    assert!(!list.names_a_list());
    // a refinement on its own does not name a list
    let (_, list) = read("category=notification&include_quarantined=true&accounts=ext_a");
    assert!(!list.names_a_list());
    let (_, list) = read("archived=false&folder=NP&unread=true");
    assert!(list.names_a_list());
    assert_eq!(list.filter().folder.as_deref(), Some("NP"));
}

/// Review is its own flag, as it is for search.
#[test]
fn review_is_its_own_flag() {
    let (review, _) = read("archived=false&quarantined=true");
    assert!(review.quarantined);
}
