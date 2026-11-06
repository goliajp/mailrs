//! The search query string, checked against the same fixture as the web
//! client's `listPath`; `request_contract_registry.rs` lists the fixture in
//! `every_fixture_has_a_test`.
//!
//! Search is a GET, so its fixture is the query parameters, as strings,
//! rather than a body. It names every list axis: the list and the search
//! inside it are read by one struct, and an axis the search drops answers
//! a question about the whole mailbox while the heading names one list.

mod common;
use axum::extract::Query;
use axum::http::Uri;
use mailrs_webapi::handlers::conversations::ListQuery;
use mailrs_webapi::handlers::misc::SearchQuery;

fn uri(params: &serde_json::Map<String, serde_json::Value>) -> Uri {
    let pairs: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{k}={}", v.as_str().expect("a string")))
        .collect();
    format!("/api/conversations/search?{}", pairs.join("&"))
        .parse()
        .expect("uri")
}

#[test]
fn search_query_matches() {
    let params: serde_json::Map<String, serde_json::Value> = common::parse("conversation-search");
    let uri = uri(&params);
    let Query(search) = Query::<SearchQuery>::try_from_uri(&uri).expect("search params");
    let Query(list) = Query::<ListQuery>::try_from_uri(&uri).expect("list params");
    assert_eq!(search.q, "invoice");
    assert!(!search.quarantined);
    let f = list.filter();
    assert_eq!(f.limit, 50);
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

/// Review is searched by its own flag, without the list's parameters.
#[test]
fn a_search_in_review_is_its_own_flag() {
    let uri: Uri = "/api/conversations/search?q=invoice&limit=50&quarantined=true"
        .parse()
        .expect("uri");
    let Query(search) = Query::<SearchQuery>::try_from_uri(&uri).expect("search params");
    assert!(search.quarantined);
}

/// Why the web client sends `true`: a boolean parameter given as `1` is
/// refused, and the whole list or search with it.
#[test]
fn a_one_is_not_a_boolean() {
    let uri: Uri = "/api/conversations?include_quarantined=1"
        .parse()
        .expect("uri");
    assert!(Query::<ListQuery>::try_from_uri(&uri).is_err());
}
