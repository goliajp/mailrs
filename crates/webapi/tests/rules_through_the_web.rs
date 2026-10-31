//! Rules through the web: refused with a reason, previewed, run over mail
//! already received — changing as many conversations as the preview said —
//! and undone.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-rules-through-the-web";
const ALICE: &str = "alice@example.test";
const PW: &str = "pw";

fn row<'a>(list: &'a Value, tid: &str) -> Option<&'a Value> {
    list.as_array()?.iter().find(|r| r["thread_id"] == tid)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rule_is_previewed_run_and_undone() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, PW).await;
    let alice = live.login(ALICE, PW).await;
    live.mailbox.ensure_thread_table();
    for (tid, from, date) in [
        ("t-shop-1", "news@shop.example", 1_786_000_000),
        ("t-shop-2", "deals@shop.example", 1_786_000_100),
        ("t-friend", "friend@partner.example", 1_786_000_200),
    ] {
        live.mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
                is_own: false,
                latest_date: date,
                latest_preview: "hello",
                senders_csv: from,
                subject: "hello",
                thread_id: tid,
                unread: true,
                user: ALICE,
            })
            .expect("arrival");
    }
    // one already starred: undoing must leave it starred
    live.mailbox.set_starred(ALICE, "t-shop-2", true).unwrap();
    let call = |m: Method, p: &str, b: Option<Value>| {
        let (alice, live) = (alice.clone(), &live);
        let p = p.to_string();
        async move { live.call(m, &p, &alice, b).await }
    };

    for (body, code) in [
        (
            json!({"name": "x", "query": "in:inbox", "actions": ["read"]}),
            "operator_not_for_rules",
        ),
        (
            json!({"name": "x", "query": " ", "actions": ["read"]}),
            "empty_query",
        ),
        (
            json!({"name": "x", "query": "from:a", "actions": ["forward"]}),
            "invalid_action",
        ),
        (
            json!({"name": "x", "query": "from:a", "actions": ["label:99"]}),
            "invalid_action",
        ),
        (
            json!({"name": " ", "query": "from:a", "actions": ["read"]}),
            "invalid_name",
        ),
    ] {
        let (status, got) = call(Method::POST, "/api/rules", Some(body)).await;
        assert_eq!((status, got["error"].as_str()), (400, Some(code)));
    }

    let (status, preview) = call(
        Method::POST,
        "/api/rules/preview",
        Some(json!({"query": "from:shop.example"})),
    )
    .await;
    assert_eq!(status, 200, "{preview}");
    assert_eq!(preview["count"], 2);

    let (status, rule) = call(
        Method::POST,
        "/api/rules",
        Some(json!({"name": "Shop", "query": "from:shop.example", "actions": ["archive", "star", "archive"]})),
    )
    .await;
    assert_eq!(status, 201, "{rule}");
    assert_eq!(rule["actions"], json!(["archive", "star"]));
    let id = rule["id"].as_str().unwrap().to_string();
    let (status, _) = call(
        Method::POST,
        "/api/rules",
        Some(json!({"name": "shop", "query": "from:x", "actions": ["read"]})),
    )
    .await;
    assert_eq!(status, 409);

    let (status, run) = call(Method::POST, &format!("/api/rules/{id}/apply"), None).await;
    assert_eq!(status, 200, "{run}");
    assert_eq!(
        (run["changed"].as_u64(), run["failed"].as_u64()),
        (Some(2), Some(0))
    );
    let (_, inbox) = call(Method::GET, "/api/conversations?limit=50", None).await;
    assert!(row(&inbox, "t-shop-1").is_none(), "archived: {inbox}");
    assert!(row(&inbox, "t-friend").is_some());
    let (_, archived) = call(
        Method::GET,
        "/api/conversations?limit=50&archived=true",
        None,
    )
    .await;
    for tid in ["t-shop-1", "t-shop-2"] {
        assert_eq!(
            row(&archived, tid).expect(tid)["flagged"],
            true,
            "{archived}"
        );
    }

    let run_id = run["run_id"].as_str().unwrap();
    let (status, undone) = call(
        Method::POST,
        &format!("/api/rules/runs/{run_id}/undo"),
        None,
    )
    .await;
    assert_eq!(
        (status, undone["restored"].as_u64()),
        (200, Some(2)),
        "{undone}"
    );
    let (_, inbox) = call(Method::GET, "/api/conversations?limit=50", None).await;
    assert_eq!(row(&inbox, "t-shop-1").expect("back")["flagged"], false);
    assert_eq!(row(&inbox, "t-shop-2").expect("back")["flagged"], true);
    // once
    let (status, _) = call(
        Method::POST,
        &format!("/api/rules/runs/{run_id}/undo"),
        None,
    )
    .await;
    assert_eq!(status, 404);

    // MCP reaches the same rules
    let mut mcp = live.mcp(&alice).await;
    let preview = mcp
        .tool("preview_rule", json!({"query": "from:partner.example"}))
        .await
        .expect("preview_rule");
    assert_eq!(preview["count"], 1);
    let made = mcp
        .tool(
            "create_rule",
            json!({"name": "Friend", "query": "from:partner.example", "actions": ["star"]}),
        )
        .await
        .expect("create_rule");
    let refused = mcp
        .tool(
            "create_rule",
            json!({"name": "Bad", "query": "is:unread", "actions": ["star"]}),
        )
        .await
        .expect_err("refused");
    assert!(
        refused["message"]
            .as_str()
            .unwrap_or("")
            .contains("operator_not_for_rules")
            || refused.to_string().contains("operator_not_for_rules"),
        "{refused}"
    );
    let listed = mcp.tool("list_rules", json!({})).await.expect("list_rules");
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);
    mcp.tool("delete_rule", json!({"rule_id": made["id"]}))
        .await
        .expect("delete_rule");

    let (_, list) = call(Method::GET, "/api/rules", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (status, _) = call(Method::DELETE, &format!("/api/rules/{id}"), None).await;
    assert_eq!(status, 204);
}
