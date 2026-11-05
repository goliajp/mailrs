//! Every write to an email group, a permission group or an account leaves
//! exactly one audit row naming the administrator, whether it came through
//! the admin REST routes or an MCP tool. Accounts are recorded by the core
//! that made or removed them; webapi passes the administrator along, so the
//! row is not written twice.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-group-and-account-writes-are-audited";
const ADMIN: &str = "admin@example.test";

fn rows() -> Vec<Value> {
    let mut c = kevy_client::Connection::connect(KEVY).unwrap();
    c.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<Value>(r).unwrap())
        .collect()
}

/// The one row for `action` on `target` with `detail`, which must be by
/// the admin.
fn one(action: &str, target: &str, detail: &str) {
    let all = rows();
    let got: Vec<&Value> = all
        .iter()
        .filter(|r| r["action"] == action && r["target"] == target && r["detail"] == detail)
        .collect();
    assert_eq!(got.len(), 1, "{action} {target} {detail}: {all:#?}");
    assert_eq!(got[0]["actor"], ADMIN, "{action} {target}");
}

/// The one row for `action` on `target`, whatever its detail, which must be
/// by the admin. A soft delete records the group it removed as its detail.
fn one_on(action: &str, target: &str) {
    let all = rows();
    let got: Vec<&Value> = all
        .iter()
        .filter(|r| r["action"] == action && r["target"] == target)
        .collect();
    assert_eq!(got.len(), 1, "{action} {target}: {all:#?}");
    assert_eq!(got[0]["actor"], ADMIN, "{action} {target}");
}

/// How many rows `action` has in all, so a second row with another detail
/// cannot hide beside the one [`one`] found.
fn count(action: &str) -> usize {
    rows().iter().filter(|r| r["action"] == action).count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn group_and_account_writes_leave_one_row_each() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ADMIN, "pw").await;
    live.make_super(ADMIN);
    let token = live.login(ADMIN, "pw").await;
    live.confirm(&token, "pw").await;
    let call = |method: Method, path: String, body: Option<Value>| {
        let (live, token) = (&live, token.clone());
        async move { live.call(method, &path, &token, body).await }
    };

    // email groups over REST
    let (status, g) = call(
        Method::POST,
        "/api/admin/email-groups".into(),
        Some(json!({ "address": "team@example.test", "domain": "example.test", "name": "Team" })),
    )
    .await;
    assert_eq!(status, 200, "{g}");
    let id = g["id"].as_i64().unwrap().to_string();
    let (status, _) = call(
        Method::POST,
        format!("/api/admin/email-groups/{id}/members"),
        Some(json!({ "address": "a@example.test" })),
    )
    .await;
    assert_eq!(status, 204);
    let (status, _) = call(
        Method::DELETE,
        format!("/api/admin/email-groups/{id}/members/a%40example.test"),
        None,
    )
    .await;
    assert_eq!(status, 204);
    let (status, _) = call(
        Method::DELETE,
        format!("/api/admin/email-groups/{id}"),
        None,
    )
    .await;
    assert_eq!(status, 204);
    one("email_group.create", &id, "team@example.test");
    one("email_group.member_add", &id, "a@example.test");
    one("email_group.member_remove", &id, "a@example.test");
    one_on("email_group.soft_deleted", &id);

    // email groups over MCP
    let mut mcp = live.mcp(&token).await;
    let g = mcp
        .tool(
            "create_email_group",
            json!({ "address": "desk@example.test", "name": "Desk" }),
        )
        .await
        .expect("create");
    let mid = g["id"].as_i64().unwrap();
    let member = json!({ "group_id": mid.to_string(), "address": "b@example.test" });
    mcp.tool("add_email_group_member", member.clone())
        .await
        .expect("add member");
    mcp.tool("remove_email_group_member", member)
        .await
        .expect("remove member");
    mcp.tool("delete_email_group", json!({ "id": mid }))
        .await
        .expect("delete");
    let mid = mid.to_string();
    one("email_group.create", &mid, "desk@example.test");
    one("email_group.member_add", &mid, "b@example.test");
    one("email_group.member_remove", &mid, "b@example.test");
    one_on("email_group.soft_deleted", &mid);

    // permission groups over REST
    let (status, pg) = call(
        Method::POST,
        "/api/admin/groups".into(),
        Some(json!({ "name": "ops" })),
    )
    .await;
    assert_eq!(status, 200, "{pg}");
    let pid = pg["id"].as_i64().unwrap().to_string();
    let (status, _) = call(
        Method::PUT,
        format!("/api/admin/groups/{pid}/permissions"),
        Some(json!({ "permissions": ["admin.queue", "admin.sieve"] })),
    )
    .await;
    assert_eq!(status, 204);
    let (status, _) = call(
        Method::POST,
        format!("/api/admin/groups/{pid}/members"),
        Some(json!({ "address": "c@example.test" })),
    )
    .await;
    assert_eq!(status, 204);
    let (status, _) = call(
        Method::DELETE,
        format!("/api/admin/groups/{pid}/members/c%40example.test"),
        None,
    )
    .await;
    assert_eq!(status, 204);
    // membership over MCP
    let member = json!({ "group_id": pid.parse::<i64>().unwrap(), "address": "d@example.test" });
    mcp.tool("add_account_to_group", member.clone())
        .await
        .expect("add to group");
    mcp.tool("remove_account_from_group", member)
        .await
        .expect("remove from group");
    let (status, _) = call(Method::DELETE, format!("/api/admin/groups/{pid}"), None).await;
    assert_eq!(status, 204);
    one("group.create", &pid, "ops");
    one("group.permissions_set", &pid, "admin.queue,admin.sieve");
    one("group.member_add", &pid, "c@example.test");
    one("group.member_remove", &pid, "c@example.test");
    one("group.member_add", &pid, "d@example.test");
    one("group.member_remove", &pid, "d@example.test");
    one("group.delete", &pid, "");

    // accounts over REST, then over MCP
    let (status, _) = call(
        Method::POST,
        "/api/admin/accounts".into(),
        Some(
            json!({ "address": "dan@example.test", "display_name": "Dan",
                     "password": "a long enough passphrase 1" }),
        ),
    )
    .await;
    assert_eq!(status, 204);
    let (status, _) = call(
        Method::DELETE,
        "/api/admin/accounts/dan%40example.test".into(),
        None,
    )
    .await;
    assert_eq!(status, 204);
    one("account.create", "dan@example.test", "");
    one("account.delete", "dan@example.test", "");
    mcp.tool(
        "create_account",
        json!({ "address": "eve@example.test", "display_name": "Eve",
                "password": "a long enough passphrase 2" }),
    )
    .await
    .expect("create account");
    mcp.tool("remove_account", json!({ "address": "eve@example.test" }))
        .await
        .expect("remove account");
    one("account.create", "eve@example.test", "");
    one("account.delete", "eve@example.test", "");

    // the admin's own account was made by the core directly, as `system`
    let totals = [
        ("email_group.create", 2),
        ("email_group.member_add", 2),
        ("email_group.member_remove", 2),
        ("email_group.soft_deleted", 2),
        ("group.create", 1),
        ("group.permissions_set", 1),
        ("group.member_add", 2),
        ("group.member_remove", 2),
        ("group.delete", 1),
        ("account.create", 3),
        ("account.delete", 2),
    ];
    for (action, n) in totals {
        assert_eq!(count(action), n, "{action}: {:#?}", rows());
    }
}
