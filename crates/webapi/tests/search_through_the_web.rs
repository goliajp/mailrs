//! Search operators and saved searches through the web: an operator-only
//! query finds what the words alone cannot, and a saved search belongs to
//! the one who saved it.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-search-through-the-web";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const PW: &str = "pw";

fn ids(list: &Value) -> Vec<String> {
    let mut ids: Vec<String> = list
        .as_array()
        .expect("a list")
        .iter()
        .map(|r| r["thread_id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn operators_search_and_searches_are_saved() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, PW).await;
    live.add_account(BOB, PW).await;
    let alice = live.login(ALICE, PW).await;
    let bob = live.login(BOB, PW).await;
    live.mailbox.ensure_thread_table();
    for (tid, from, category) in [
        ("t-friend", "friend@partner.example", "inbox"),
        ("t-shop", "news@shop.example", "promotion"),
        ("t-junk", "x@spam.example", "spam"),
    ] {
        live.mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category,
                is_own: false,
                latest_date: 1_786_000_000,
                latest_preview: "hello",
                senders_csv: from,
                subject: "hello",
                thread_id: tid,
                unread: true,
                user: ALICE,
            })
            .expect("arrival");
    }
    let call = |who: &str, m: Method, p: &str, b: Option<Value>| {
        let (who, live) = (who.to_string(), &live);
        let p = p.to_string();
        async move { live.call(m, &p, &who, b).await }
    };
    let search = |q: &str| {
        let q = q.replace(':', "%3A").replace(' ', "+");
        call(
            &alice,
            Method::GET,
            &format!("/api/conversations/search?q={q}&folder=inbox"),
            None,
        )
    };

    let (status, found) = search("from:partner").await;
    assert_eq!(status, 200, "{found}");
    assert_eq!(ids(&found), ["t-friend"]);
    // the Inbox does not show Junk; `in:` looks where it says
    assert_eq!(ids(&search("from:spam").await.1), Vec::<String>::new());
    assert_eq!(ids(&search("in:junk").await.1), ["t-junk"]);
    assert_eq!(
        ids(&search("in:anywhere").await.1),
        ["t-friend", "t-junk", "t-shop"]
    );

    // MCP searches the same way
    let mut mcp = live.mcp(&alice).await;
    let hits = mcp
        .tool(
            "search_conversations",
            json!({"q": "in:anywhere -from:partner"}),
        )
        .await
        .expect("search_conversations");
    let mut found: Vec<&str> = hits["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["thread_id"].as_str().unwrap())
        .collect();
    found.sort();
    assert_eq!(found, ["t-junk", "t-shop"]);

    let (status, saved) = call(
        &alice,
        Method::POST,
        "/api/saved-searches",
        Some(json!({"name": " Partners ", "query": "from:partner"})),
    )
    .await;
    assert_eq!(status, 201, "{saved}");
    assert_eq!(saved["name"], "Partners");
    let id = saved["id"].as_str().unwrap().to_string();
    let (status, body) = call(
        &alice,
        Method::POST,
        "/api/saved-searches",
        Some(json!({"name": "partners", "query": "x"})),
    )
    .await;
    assert_eq!((status, body["error"].as_str()), (409, Some("name_taken")));
    let (status, body) = call(
        &alice,
        Method::POST,
        "/api/saved-searches",
        Some(json!({"name": "empty", "query": "  "})),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("invalid_query"))
    );

    let (_, list) = call(&alice, Method::GET, "/api/saved-searches", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list[0]["query"], "from:partner");
    // Bob sees none of Alice's, and cannot delete hers
    let (_, theirs) = call(&bob, Method::GET, "/api/saved-searches", None).await;
    assert_eq!(theirs, json!([]));
    let (status, _) = call(
        &bob,
        Method::DELETE,
        &format!("/api/saved-searches/{id}"),
        None,
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = call(
        &alice,
        Method::DELETE,
        &format!("/api/saved-searches/{id}"),
        None,
    )
    .await;
    assert_eq!(status, 204);
    let (_, list) = call(&alice, Method::GET, "/api/saved-searches", None).await;
    assert_eq!(list, json!([]));
}
