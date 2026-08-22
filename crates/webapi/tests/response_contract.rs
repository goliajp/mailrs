//! What the handlers send, against the shape the client parses.
//!
//! The mirror of `request_contract.rs`. Each fixture in
//! `wire-contract/responses/` was captured from production; this asserts the
//! handler's own response type still serializes to that key set, and
//! `web/src/wire/__tests__/response-contract.test.ts` asserts the client's
//! Zod schema still parses it. Neither side can pass by agreeing with
//! itself, which is the failure that let nine request bodies be wrong on
//! 2026-07-30 while every test stayed green.
//!
//! Keys, not values: the fixture's values are synthetic on purpose (a real
//! capture would commit somebody's mail), and a renamed or dropped field is
//! what breaks a client.

use std::collections::BTreeSet;

use mailrs_webapi::handlers;

fn fixture(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/../../wire-contract/responses/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

/// The key set of an object, or of the first element of an array.
fn keys(v: &serde_json::Value) -> BTreeSet<String> {
    let obj = match v {
        serde_json::Value::Array(items) => items.first().expect("fixture array is empty"),
        other => other,
    };
    obj.as_object()
        .unwrap_or_else(|| panic!("not an object: {obj}"))
        .keys()
        .cloned()
        .collect()
}

#[test]
fn conversation_list_keys_match() {
    let sample = handlers::conversations::ConversationResponse {
        thread_id: "t1".into(),
        subject: "s".into(),
        participants: vec!["a@b.com".into()],
        message_count: 1,
        unread_count: 0,
        last_date: 0,
        category: "inbox".into(),
        flagged: false,
        snippet: "".into(),
        pinned: false,
        archived: false,
        importance_level: "normal".into(),
        importance_score: 0.0,
        requires_action: false,
        received_count: 1,
        sent_count: 0,
        account_id: String::new(),
    };
    let serialized = serde_json::to_value(&sample).expect("serialize");

    let from_handler = keys(&serialized);
    let from_prod = keys(&fixture("conversation-list"));
    assert_eq!(
        from_handler, from_prod,
        "the conversation list's fields have changed since the fixture was \
         captured. If that is intended, re-capture the fixture and update \
         the Zod schema in web/src/wire/schemas/conversation.ts — the client \
         drops what it does not name."
    );
}

#[test]
fn conversation_categories_keys_match() {
    let from_prod = keys(&fixture("conversation-categories"));
    // The handler builds this inline rather than from a named struct, so
    // the fixture is the only statement of its shape. Pinning the two keys
    // here means a rename shows up as a failing test rather than as empty
    // filter chips.
    assert_eq!(
        from_prod,
        ["category", "count"]
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>()
    );
}

#[test]
fn send_list_keys_match() {
    let from_prod = keys(&fixture("send-list"));
    let expected: BTreeSet<String> = [
        "can_resend",
        "created_at",
        "recipients",
        "resent_from",
        "send_id",
        "status",
        "subject",
        "thread_id",
        "to",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        from_prod, expected,
        "the Send projection's fields have changed. `can_resend` and \
         `resent_from` drive the Send tab's re-edit affordance; losing \
         either silently removes the button."
    );

    // `resent_from` is null in the fixture and must stay nullable: a send
    // that is not a resend has no origin, and a schema that required it
    // would reject every ordinary send.
    let first = &fixture("send-list")[0];
    assert!(first["resent_from"].is_null());
    // The per-recipient rows are what the status column reads.
    let r = &first["recipients"][0];
    for k in ["recipient", "delivered", "pending", "code", "message"] {
        assert!(r.get(k).is_some(), "recipient row lost `{k}`");
    }
}

/// What answering an invitation sends back.
///
/// `message` is **absent** on success, not null. `Option<String>`
/// serialises to `null` by default and the client's schema declares it
/// `z.string().optional()`, which admits a missing key and refuses a
/// null one — so every successful RSVP came back failing validation and
/// the card printed "Response failed validation (1 issue)" under
/// buttons that had just worked. Found in production, on the first
/// answer anybody sent.
#[test]
fn an_rsvp_result_says_nothing_rather_than_saying_null() {
    let ok = handlers::invites::RsvpResponse {
        success: true,
        message: None,
    };
    let json = serde_json::to_value(&ok).expect("serialises");
    assert_eq!(
        keys(&json),
        keys(&fixture("rsvp-result")),
        "the success shape drifted from the fixture the client parses"
    );
    assert!(
        json.get("message").is_none(),
        "message must be absent, not null: {json}"
    );

    // And when there is something to say, it is a string.
    let failed = handlers::invites::RsvpResponse {
        success: false,
        message: Some("the reply could not be queued".into()),
    };
    let json = serde_json::to_value(&failed).expect("serialises");
    assert_eq!(
        json.get("message").and_then(|m| m.as_str()),
        Some("the reply could not be queued")
    );
}

/// Every fixture is checked by a test above.
#[test]
fn every_response_fixture_has_a_test() {
    const CHECKED: &[&str] = &[
        "conversation-categories",
        "conversation-list",
        "rsvp-result",
        "send-list",
    ];
    let dir = format!(
        "{}/../../wire-contract/responses",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {dir}: {e}"))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".json").map(str::to_string)
        })
        .collect();
    found.sort();
    let mut checked: Vec<String> = CHECKED.iter().map(|s| s.to_string()).collect();
    checked.sort();
    assert_eq!(
        found, checked,
        "a fixture with no test is a file that looks like coverage and is not"
    );
}
