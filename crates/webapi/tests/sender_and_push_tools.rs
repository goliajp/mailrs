//! The sender rules and push settings an agent reaches over MCP are the
//! ones the web reads and writes: what a tool sets, the route returns, and
//! what the route refuses, the tool refuses.

mod live;

use reqwest::Method;
use serde_json::json;

const KEVY: &str = "mem://webapi-sender-and-push-tools";
const ALICE: &str = "alice@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sender_rules_and_push_settings_through_mcp() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let call = |m: Method, path: &'static str| {
        let (live, alice) = (&live, alice.clone());
        async move { live.call(m, path, &alice, None).await }
    };
    let mut mcp = live.mcp(&alice).await;

    mcp.tool(
        "set_important_contact",
        json!({"address": "Boss <boss@example.test>", "important": true}),
    )
    .await
    .expect("set_important_contact");
    let (_, vips) = call(Method::GET, "/api/contacts/important").await;
    assert_eq!(vips, json!(["boss@example.test"]));
    let listed = mcp
        .tool("list_important_contacts", json!({}))
        .await
        .expect("list_important_contacts");
    assert_eq!(listed["items"], vips);

    mcp.tool(
        "set_notify_settings",
        json!({"mode": "important", "quiet_start": 1320, "quiet_end": 420, "utc_offset_minutes": 540}),
    )
    .await
    .expect("set_notify_settings");
    let (_, prefs) = call(Method::GET, "/api/notify").await;
    assert_eq!(prefs["mode"], "important");
    assert_eq!(prefs["quiet_start"], 1320);
    mcp.tool("set_notify_settings", json!({"mode": "sometimes"}))
        .await
        .expect_err("an unknown mode is refused");
    mcp.tool(
        "set_notify_settings",
        json!({"mode": "all", "quiet_start": 60}),
    )
    .await
    .expect_err("quiet hours need both ends, as the route says");

    mcp.tool(
        "set_thread_notify",
        json!({"thread_id": "t-1", "value": "mute"}),
    )
    .await
    .expect("set_thread_notify");
    mcp.tool(
        "set_sender_notify",
        json!({"address": "news@example.test", "value": "never"}),
    )
    .await
    .expect("set_sender_notify");
    let got = mcp
        .tool("get_notify_settings", json!({}))
        .await
        .expect("get_notify_settings");
    assert_eq!(got["prefs"]["mode"], "important");
    assert_eq!(got["overrides"]["threads"], json!([["t-1", "mute"]]));
    assert_eq!(
        got["overrides"]["senders"],
        json!([["news@example.test", "never"]])
    );
    let stats = mcp
        .tool("notify_stats", json!({"days": 3}))
        .await
        .expect("notify_stats");
    assert_eq!(stats["items"].as_array().map(Vec::len), Some(3));

    mcp.tool(
        "set_sender_rule",
        json!({"address": "news@example.test", "bucket": "promotions"}),
    )
    .await
    .expect("set_sender_rule");
    let (_, rules) = call(Method::GET, "/api/sender-rules").await;
    assert_eq!(rules[0]["address"], "news@example.test");
    assert_eq!(rules[0]["bucket"], "promotions");
    mcp.tool(
        "set_sender_rule",
        json!({"address": "news@example.test", "bucket": "elsewhere"}),
    )
    .await
    .expect_err("an unknown bucket is refused");
    let listed = mcp
        .tool("list_sender_rules", json!({}))
        .await
        .expect("list_sender_rules");
    assert_eq!(listed["items"], rules);
    mcp.tool(
        "delete_sender_rule",
        json!({"address": "news@example.test"}),
    )
    .await
    .expect("delete_sender_rule");
    let (_, rules) = call(Method::GET, "/api/sender-rules").await;
    assert_eq!(rules, json!([]));

    let moved = mcp
        .tool(
            "reclassify_sender",
            json!({"sender": "shop@example.test", "bucket": "promotions"}),
        )
        .await
        .expect("reclassify_sender");
    assert_eq!(
        moved,
        json!({"moved": 0}),
        "nothing to move, nothing to undo"
    );
    mcp.tool(
        "undo_sender_correction",
        json!({"correction_id": "no-such"}),
    )
    .await
    .expect_err("an unknown correction is not found");

    let addresses = json!([
        "New@Example.test",
        "new@example.test ",
        "other@example.test"
    ]);
    let accepted = mcp
        .tool("accept_senders", json!({"addresses": addresses}))
        .await
        .expect("accept_senders");
    let (status, routed) = live
        .call(
            Method::POST,
            "/api/senders/accept",
            &alice,
            Some(json!({"addresses": addresses})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(accepted, routed, "the tool answers what the route answers");
    assert_eq!(
        accepted["accepted"], 2,
        "one address, spelled twice, counts once"
    );
    mcp.tool("accept_senders", json!({"addresses": []}))
        .await
        .expect_err("an empty list is refused, as the route refuses it");

    // a key that may only read reaches the reading tools and none of the writing ones
    live.confirm(&alice, "pw").await;
    let (status, created) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &alice,
            Some(json!({"name": "reader", "scopes": ["mail:read"]})),
        )
        .await;
    assert_eq!(status, 200, "{created}");
    let reader = created["secret"].as_str().expect("secret").to_string();
    let mut ro = live.mcp(&reader).await;
    ro.tool("list_sender_rules", json!({}))
        .await
        .expect("a reading tool is open to a mail:read key");
    for (name, args) in [
        (
            "set_sender_rule",
            json!({"address": "x@example.test", "bucket": "inbox"}),
        ),
        ("accept_senders", json!({"addresses": ["x@example.test"]})),
        (
            "set_important_contact",
            json!({"address": "x@example.test", "important": true}),
        ),
    ] {
        ro.tool(name, args)
            .await
            .expect_err("a writing tool is refused to a mail:read key");
    }
    let (_, rules) = call(Method::GET, "/api/sender-rules").await;
    let written = rules
        .as_array()
        .expect("a list")
        .iter()
        .any(|r| r["address"] == "x@example.test");
    assert!(!written, "the refused call wrote nothing: {rules}");
    let (_, vips) = call(Method::GET, "/api/contacts/important").await;
    assert_eq!(
        vips,
        json!(["boss@example.test"]),
        "nor marked anyone important"
    );
}
