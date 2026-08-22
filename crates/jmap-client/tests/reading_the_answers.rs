//! Reading what a JMAP server said.
//!
//! The requests are JSON this builds and the answers are JSON it
//! reads; neither needs a network to test, and the parts that go wrong
//! are all in here.

use mailrs_jmap_client::{Changes, blob_url, parse_changes, parse_session};

const SESSION: &str = r#"{
  "apiUrl": "https://api.fastmail.com/jmap/api/",
  "downloadUrl": "https://api.fastmail.com/jmap/download/{accountId}/{blobId}/{name}",
  "primaryAccounts": { "urn:ietf:params:jmap:mail": "u33e4f7b" },
  "accounts": { "u33e4f7b": { "name": "me@fastmail.com" } },
  "state": "cyrus-0;p-5"
}"#;

#[test]
fn the_session_gives_the_endpoint_rather_than_the_url_somebody_typed() {
    let s = parse_session(SESSION).expect("a session");
    assert_eq!(s.api_url, "https://api.fastmail.com/jmap/api/");
    assert_eq!(s.account_id, "u33e4f7b");
}

/// A provider may move its endpoint — Fastmail has — so an account
/// built on the URL a person typed works until it does not.
#[test]
fn a_session_without_a_mail_account_is_refused_rather_than_guessed_at() {
    let no_mail = r#"{"apiUrl":"x","downloadUrl":"y","primaryAccounts":{},"accounts":{}}"#;
    assert!(parse_session(no_mail).is_none());
    assert!(parse_session("not json").is_none());
    assert!(parse_session("{}").is_none());
}

#[test]
fn a_download_url_is_filled_in_from_its_template() {
    let s = parse_session(SESSION).unwrap();
    let url = blob_url(&s, "Gabcdef", "message.eml");
    assert_eq!(
        url,
        "https://api.fastmail.com/jmap/download/u33e4f7b/Gabcdef/message.eml"
    );
}

/// A blob id may contain characters a URL path cannot carry verbatim.
#[test]
fn a_blob_id_is_escaped_into_the_url() {
    let s = parse_session(SESSION).unwrap();
    assert!(blob_url(&s, "a/b c", "n").contains("a%2Fb%20c"));
}

/// The one that silently stops the sync.
///
/// `cannotCalculateChanges` means the server can no longer say what
/// changed since that state — the only correct answer is to read the
/// mailbox again. A client that treats it as an error to log keeps
/// asking from a state the server has forgotten and never sees another
/// message.
#[test]
fn cannot_calculate_changes_is_start_over_not_an_error() {
    let answer = r#"{"methodResponses":[["error",{"type":"cannotCalculateChanges"},"c"]]}"#;
    assert!(matches!(parse_changes(answer), Some(Changes::StartOver)));
}

#[test]
fn a_changes_answer_gives_what_moved_and_the_new_state() {
    let answer = r#"{"methodResponses":[["Email/changes",{
        "accountId":"u1","oldState":"s1","newState":"s2","hasMoreChanges":false,
        "created":["M1","M2"],"updated":[],"destroyed":["M0"]},"c"]]}"#;
    let Some(Changes::Moved {
        created,
        destroyed,
        new_state,
        has_more,
    }) = parse_changes(answer)
    else {
        panic!("not read as changes");
    };
    assert_eq!(created, vec!["M1", "M2"]);
    assert_eq!(destroyed, vec!["M0"]);
    assert_eq!(new_state, "s2");
    assert!(!has_more);
}

/// `hasMoreChanges` means this answer is a page, not the whole story.
/// Ignoring it stops the sync one page in, which looks like nothing
/// new arriving.
#[test]
fn more_changes_pending_is_carried_through() {
    let answer = r#"{"methodResponses":[["Email/changes",{
        "accountId":"u1","oldState":"s1","newState":"s2","hasMoreChanges":true,
        "created":[],"updated":[],"destroyed":[]},"c"]]}"#;
    let Some(Changes::Moved { has_more, .. }) = parse_changes(answer) else {
        panic!("not read as changes");
    };
    assert!(has_more);
}

#[test]
fn nonsense_is_refused_rather_than_panicking() {
    for junk in [
        "",
        "{}",
        "[]",
        r#"{"methodResponses":[]}"#,
        r#"{"methodResponses":[["x",{},"c"]]}"#,
    ] {
        let _ = parse_changes(junk);
    }
}
