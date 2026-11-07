//! `subject`, `from` and `to` in an `Email/query` filter look in their own
//! header, not anywhere a full-text search would match (RFC 8621 §4.4.1).

use serde_json::json;

use super::{ALICE, import, live, message, one};

pub(super) async fn header_filters_look_in_their_header(live: &live::Live, alice: &str) {
    let one_id = import(
        live,
        alice,
        ALICE,
        &message("zebra probe one", "Zed <zed@zebra.example>", "nothing here"),
    )
    .await;
    let race_id = import(
        live,
        alice,
        ALICE,
        &message(
            "zebra race probe",
            "Rae <rae@race.example>",
            "zebra probe one, quoted",
        ),
    )
    .await;
    let ids = |r: serde_json::Value| -> Vec<String> {
        r["ids"]
            .as_array()
            .unwrap_or_else(|| panic!("{r}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    // the other message shares words with it, and quotes it in its body
    let r = one(
        live,
        alice,
        json!(["Email/query", { "filter": { "subject": "Zebra Probe One" } }, "0"]),
    )
    .await;
    assert_eq!(ids(r), [one_id.as_str()]);
    let r = one(
        live,
        alice,
        json!(["Email/query", { "filter": { "from": "race.example" } }, "0"]),
    )
    .await;
    assert_eq!(ids(r), [race_id.as_str()]);
    // the body is still what `text` searches
    let r = one(
        live,
        alice,
        json!(["Email/query", { "filter": { "text": "quoted" } }, "0"]),
    )
    .await;
    assert_eq!(ids(r), [race_id.as_str()]);
}
