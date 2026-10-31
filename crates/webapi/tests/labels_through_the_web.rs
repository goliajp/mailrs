//! Labels through the web and MCP: made, refused with a reason, put on a
//! conversation, listed by label and carried on the rows, taken off, and
//! deleted without deleting mail.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-labels-through-the-web";
const ALICE: &str = "alice@example.test";
const PW: &str = "pw";

fn rows(list: &Value) -> Vec<(String, Value)> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|r| {
            (
                r["thread_id"].as_str().unwrap().to_string(),
                r["labels"].clone(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn labels_are_made_put_on_listed_and_deleted() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, PW).await;
    let alice = live.login(ALICE, PW).await;
    live.mailbox.ensure_thread_table();
    for (tid, date) in [("t-1", 1_786_000_000), ("t-2", 1_786_000_100)] {
        live.mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
                is_own: false,
                latest_date: date,
                latest_preview: "hello",
                senders_csv: "friend@partner.example",
                subject: "hello",
                thread_id: tid,
                unread: true,
                user: ALICE,
            })
            .expect("arrival");
    }
    let call = |m: Method, p: &str, b: Option<Value>| {
        let (alice, live) = (alice.clone(), &live);
        let p = p.to_string();
        async move { live.call(m, &p, &alice, b).await }
    };

    let (status, work) = call(
        Method::POST,
        "/api/labels",
        Some(json!({"name": " Work ", "color": "blue"})),
    )
    .await;
    assert_eq!(status, 201, "{work}");
    assert_eq!(
        (work["id"].as_str(), work["name"].as_str()),
        (Some("1"), Some("Work"))
    );
    let (status, body) = call(
        Method::POST,
        "/api/labels",
        Some(json!({"name": "work", "color": "red"})),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "there is already a label called work");
    let (status, body) = call(
        Method::POST,
        "/api/labels",
        Some(json!({"name": "a/b", "color": "red"})),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "a label name cannot contain /");

    let (status, _) = call(Method::POST, "/api/conversations/t-1/labels/1", None).await;
    assert_eq!(status, 204);
    let (_, list) = call(Method::GET, "/api/conversations?label=1", None).await;
    assert_eq!(rows(&list), [("t-1".to_string(), json!(["1"]))]);
    let (_, inbox) = call(Method::GET, "/api/conversations?folder=Inbox", None).await;
    assert_eq!(
        rows(&inbox),
        [
            ("t-2".to_string(), json!([])),
            ("t-1".to_string(), json!(["1"]))
        ],
        "a labelled conversation stays in the Inbox, carrying the label"
    );

    let mut mcp = live.mcp(&alice).await;
    let home = mcp
        .tool("create_label", json!({"name": "Home", "color": "green"}))
        .await
        .expect("create_label");
    assert_eq!(home["id"], "2");
    mcp.tool("label_thread", json!({"thread_id": "t-2", "label_id": "2"}))
        .await
        .expect("label_thread");
    let listed = mcp
        .tool("list_labels", json!({}))
        .await
        .expect("list_labels");
    let names: Vec<_> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].clone())
        .collect();
    assert_eq!(names, [json!("Home"), json!("Work")]);
    let (_, list) = call(Method::GET, "/api/conversations?label=2", None).await;
    assert_eq!(rows(&list), [("t-2".to_string(), json!(["2"]))]);

    let (status, _) = call(Method::DELETE, "/api/conversations/t-1/labels/1", None).await;
    assert_eq!(status, 204);
    let (_, list) = call(Method::GET, "/api/conversations?label=1", None).await;
    assert!(rows(&list).is_empty());
    let (status, _) = call(Method::DELETE, "/api/labels/2", None).await;
    assert_eq!(status, 204);
    let (_, inbox) = call(Method::GET, "/api/conversations?folder=Inbox", None).await;
    assert_eq!(
        inbox.as_array().unwrap().len(),
        2,
        "deleting a label deleted mail"
    );
    let (status, _) = call(Method::DELETE, "/api/labels/2", None).await;
    assert_eq!(status, 404);
}
