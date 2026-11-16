//! The search check: a fixed set of mail, delivered by the caller through
//! the store's own intake, and what each query must find in it.

use mailrs_core_api::method::conversation::SearchConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::SearchStore;

/// Words a body starts with, longer than any preview, so a word after
/// them is a word of the body alone.
const PAST_THE_PREVIEW: &str = "filler filler filler filler filler filler filler filler filler \
filler filler filler filler filler filler filler filler filler filler filler filler filler \
filler filler filler filler filler filler filler filler filler filler filler filler filler";

/// One letter of the set, to `to`.
fn letter(n: u32, to: &str, subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: Carol <carol@harbor.invalid>\r\nTo: {to}\r\nSubject: {subject}\r\n\
         Message-ID: <search-{n}@harbor.invalid>\r\n\
         Date: Mon, {n} Nov 2026 10:00:00 +0000\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
    )
    .into_bytes()
}

/// The mail [`check_search_store`] searches, oldest first. The caller
/// delivers every one to `to` through its intake, so bodies are indexed
/// the way arriving mail's are.
pub fn search_fixtures(to: &str) -> Vec<Vec<u8>> {
    vec![
        letter(2, to, "Visit lantern.zone tomorrow", "plain words"),
        letter(3, to, "Zone only", "nothing else here"),
        letter(
            4,
            to,
            "Calendar",
            &format!("{PAST_THE_PREVIEW} Join at meet.zone.rocks on Monday"),
        ),
        letter(
            5,
            to,
            "請求書のご案内",
            &format!("{PAST_THE_PREVIEW} 金額をご確認ください"),
        ),
        letter(
            6,
            to,
            "Quarterly",
            &format!("{PAST_THE_PREVIEW} the zebrafinch numbers"),
        ),
        letter(7, to, "Qualcomm vendor", "see attached"),
        letter(
            8,
            to,
            "Notes",
            &format!("{PAST_THE_PREVIEW} qualcommish stuff"),
        ),
    ]
}

/// What the store and the shared search answer for `query` in All Mail,
/// as subjects, best first.
async fn found<S: SearchStore>(store: &S, user: &str, query: &str) -> Vec<String> {
    let req = SearchConversationsRequest {
        query: query.into(),
        filter: ConversationFilter {
            folder: Some("all".into()),
            ..Default::default()
        },
        quarantined: false,
    };
    crate::search::search(store, user, &req)
        .await
        .unwrap_or_else(|e| panic!("{query}: {e}"))
        .into_iter()
        .map(|r| r.subject)
        .collect()
}

/// Every query over [`search_fixtures`], with what it must find, in order.
pub async fn check_search_store<S: SearchStore>(store: &S, user: &str) {
    let cases: &[(&str, &[&str])] = &[
        // a dotted address is its words, and every word must match
        ("lantern.zone", &["Visit lantern.zone tomorrow"]),
        ("LANTERN", &["Visit lantern.zone tomorrow"]),
        // a word of a body, cut from an address the same way
        ("rocks", &["Calendar"]),
        // words of the subject first, newest first, then of a body
        (
            "zone",
            &["Zone only", "Visit lantern.zone tomorrow", "Calendar"],
        ),
        ("zebrafinch", &["Quarterly"]),
        ("zebrafinch quarterly", &["Quarterly"]),
        ("zebrafinch calendar", &[]),
        // CJK in pairs of characters
        ("請求", &["請求書のご案内"]),
        ("ご確認", &["請求書のご案内"]),
        // inside a word of the subject, not of a body
        ("ualcom", &["Qualcomm vendor"]),
        // the sender is searched, as the list shows it
        (
            "harbor",
            &[
                "Notes",
                "Qualcomm vendor",
                "Quarterly",
                "請求書のご案内",
                "Calendar",
                "Zone only",
                "Visit lantern.zone tomorrow",
            ],
        ),
        ("nowhere", &[]),
    ];
    for (query, want) in cases {
        let got = found(store, user, query).await;
        assert_eq!(&got, want, "search {query:?}");
    }
}
