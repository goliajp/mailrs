//! The MCP alias tools and the admin REST routes are one surface: what one
//! writes the other lists, and a delete by source address from either side
//! is seen by both.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-mcp-aliases";
const ADMIN: &str = "admin@example.test";

async fn rest_rows(live: &live::Live, token: &str) -> serde_json::Value {
    let (status, body) = live
        .call(Method::GET, "/api/admin/aliases", token, None)
        .await;
    assert_eq!(status, 200);
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_alias_tools_read_and_write_what_rest_does() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ADMIN, "pw").await;
    live.grant(ADMIN, "admin.aliases").await;
    let token = live.login(ADMIN, "pw").await;
    let mut mcp = live.mcp(&token).await;

    let empty = mcp
        .tool("list_aliases_admin", serde_json::json!({}))
        .await
        .expect("list");
    assert_eq!(empty, serde_json::json!({ "items": [] }));

    // written over MCP, listed by REST
    let added = mcp
        .tool(
            "add_alias",
            serde_json::json!({
                "source_address": "sales@shop.test",
                "target_address": "admin@example.test",
            }),
        )
        .await
        .expect("add");
    assert_eq!(added, serde_json::json!({ "id": "sales@shop.test" }));
    let row = serde_json::json!({
        "id": "sales@shop.test",
        "source_address": "sales@shop.test",
        "target_address": "admin@example.test",
        "domain": "shop.test",
    });
    assert_eq!(
        rest_rows(&live, &token).await["items"],
        serde_json::json!([row])
    );

    // written over REST, listed by MCP with the same rows
    let (status, _) = live
        .call(
            Method::POST,
            "/api/admin/aliases",
            &token,
            Some(serde_json::json!({
                "source_address": "help@shop.test",
                "target_address": "admin@example.test",
            })),
        )
        .await;
    assert_eq!(status, 200);
    let mut via_mcp = mcp
        .tool("list_aliases_admin", serde_json::json!({}))
        .await
        .expect("list");
    let mut via_rest = rest_rows(&live, &token).await;
    let by_source = |v: &mut serde_json::Value| {
        v["items"]
            .as_array_mut()
            .expect("items")
            .sort_by_key(|r| r["source_address"].as_str().unwrap_or("").to_string());
    };
    by_source(&mut via_mcp);
    by_source(&mut via_rest);
    assert_eq!(via_mcp, via_rest);
    assert_eq!(via_mcp["items"].as_array().map(Vec::len), Some(2));

    // the source domain joins the domain list, as it does through REST
    let (_, domains) = live
        .call(Method::GET, "/api/admin/domains", &token, None)
        .await;
    assert!(
        domains.to_string().contains("shop.test"),
        "domain listed: {domains}"
    );

    // removed by source address over MCP; REST no longer lists it
    mcp.tool(
        "remove_alias",
        serde_json::json!({ "source_address": "sales@shop.test" }),
    )
    .await
    .expect("remove");
    let left = rest_rows(&live, &token).await;
    assert_eq!(left["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(left["items"][0]["source_address"], "help@shop.test");

    // an absent source is an error on both sides
    let missing = mcp
        .tool(
            "remove_alias",
            serde_json::json!({ "source_address": "sales@shop.test" }),
        )
        .await;
    assert!(missing.is_err(), "absent alias: {missing:?}");
    let (status, _) = live
        .call(
            Method::DELETE,
            "/api/admin/aliases/sales%40shop.test",
            &token,
            None,
        )
        .await;
    assert_eq!(status, 404);

    // the old numeric id is not a parameter any more
    let by_id = mcp
        .tool("remove_alias", serde_json::json!({ "id": 7 }))
        .await;
    assert!(by_id.is_err(), "numeric id refused: {by_id:?}");
}
