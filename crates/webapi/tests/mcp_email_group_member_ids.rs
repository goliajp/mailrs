//! The MCP member tools take the group id the way `create_email_group`
//! returns it, a number, and still take the string form older callers send.

mod live;

use serde_json::json;

const KEVY: &str = "mem://webapi-mcp-email-group-member-ids";
const ADMIN: &str = "admin@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_tools_take_a_number_or_its_string() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ADMIN, "pw").await;
    live.make_super(ADMIN);
    let token = live.login(ADMIN, "pw").await;
    live.confirm(&token, "pw").await;
    let mut mcp = live.mcp(&token).await;

    let g = mcp
        .tool(
            "create_email_group",
            json!({ "address": "desk@example.test", "name": "Desk" }),
        )
        .await
        .expect("create");
    let id = g["id"].as_i64().expect("numeric id");
    let text = id.to_string();

    mcp.tool(
        "add_email_group_member",
        json!({ "group_id": id, "address": "a@example.test" }),
    )
    .await
    .expect("add with a number");
    mcp.tool(
        "add_email_group_member",
        json!({ "group_id": text, "address": "b@example.test" }),
    )
    .await
    .expect("add with a string");

    let members = |got: serde_json::Value| {
        let mut items: Vec<String> = got["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        items.sort();
        items
    };
    let by_number = mcp
        .tool("get_email_group_members", json!({ "id": id }))
        .await
        .expect("members by number");
    assert_eq!(members(by_number), ["a@example.test", "b@example.test"]);
    let by_string = mcp
        .tool("get_email_group_members", json!({ "id": text }))
        .await
        .expect("members by string");
    assert_eq!(members(by_string), ["a@example.test", "b@example.test"]);

    mcp.tool(
        "remove_email_group_member",
        json!({ "group_id": id, "address": "a@example.test" }),
    )
    .await
    .expect("remove with a number");
    mcp.tool(
        "remove_email_group_member",
        json!({ "group_id": text, "address": "b@example.test" }),
    )
    .await
    .expect("remove with a string");
    let left = mcp
        .tool("get_email_group_members", json!({ "id": id }))
        .await
        .expect("members after removal");
    assert!(members(left).is_empty());

    let bad = mcp
        .tool(
            "add_email_group_member",
            json!({ "group_id": "desk", "address": "c@example.test" }),
        )
        .await;
    assert!(bad.is_err(), "a group id that is not a number: {bad:?}");
}
