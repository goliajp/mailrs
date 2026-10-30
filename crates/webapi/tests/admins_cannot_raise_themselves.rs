//! An administrator cannot become more than they are. A holder of
//! admin.accounts cannot touch a super administrator's account, by REST or
//! MCP; a holder of admin.groups cannot write admin.* into a group nor put
//! anyone into a group that carries it; a holder of admin.cache cannot add
//! themselves to an admin.accounts group over MCP; and creating an account
//! never makes a super administrator. Each refusal has a control that
//! passes, so a route refusing everything would not pass this test.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-admins-cannot-raise-themselves";
const SUPER: &str = "super@example.test";
const ACC: &str = "accounts@example.test";
const GRP: &str = "groups@example.test";
const CACHE: &str = "cache@example.test";
const PLAIN: &str = "plain@example.test";
const NEW: &str = "a much longer new passphrase";

fn enc(a: &str) -> String {
    a.replace('@', "%40")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admins_cannot_reach_past_what_they_hold() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [SUPER, ACC, GRP, CACHE, PLAIN] {
        live.add_account(who, "pw").await;
    }
    live.make_super(SUPER);
    live.grant(ACC, "admin.accounts").await;
    live.grant(GRP, "admin.groups").await;
    live.grant(CACHE, "admin.cache").await;
    let mut token = std::collections::HashMap::new();
    for who in [SUPER, ACC, GRP, CACHE] {
        let t = live.login(who, "pw").await;
        live.confirm(&t, "pw").await;
        token.insert(who, t);
    }
    let call = |who: &'static str, method: Method, path: String, body: Option<Value>| {
        let live = &live;
        let t = token[who].clone();
        async move { live.call(method, &path, &t, body).await.0 }
    };
    let password = || Some(json!({ "password": NEW }));

    // admin.accounts and a super administrator's account
    let reset = |a: &str| format!("/api/admin/accounts/{}/password", enc(a));
    assert_eq!(
        call(ACC, Method::POST, reset(PLAIN), password()).await,
        204,
        "control"
    );
    assert_eq!(call(ACC, Method::POST, reset(SUPER), password()).await, 403);
    let compromise = Some(json!({ "password": NEW, "compromised": { "revoke_key_ids": [] } }));
    assert_eq!(call(ACC, Method::POST, reset(SUPER), compromise).await, 403);
    let send_as = format!("/api/admin/accounts/{}/send-as", enc(SUPER));
    let forwards = format!("/api/admin/accounts/{}/forwards", enc(SUPER));
    assert_eq!(
        call(ACC, Method::PUT, send_as, Some(json!({ "send_as": [ACC] }))).await,
        403
    );
    assert_eq!(
        call(
            ACC,
            Method::PUT,
            forwards,
            Some(json!({ "forward_to": [ACC] }))
        )
        .await,
        403
    );
    assert_eq!(
        call(
            ACC,
            Method::DELETE,
            format!("/api/admin/accounts/{}", enc(SUPER)),
            None
        )
        .await,
        403
    );
    let mut mcp = live.mcp(&token[ACC]).await;
    let refused = mcp
        .tool(
            "set_account_password",
            json!({ "address": SUPER, "password": NEW }),
        )
        .await;
    assert!(
        refused.is_err(),
        "MCP reset a super administrator's password"
    );

    // admin.groups and what a group grants
    let (status, group) = live
        .call(
            Method::POST,
            "/api/admin/groups",
            &token[SUPER],
            Some(json!({ "name": "ops" })),
        )
        .await;
    assert_eq!(status, 200, "{group}");
    let id = group["id"].as_i64().expect("group id");
    let perms = format!("/api/admin/groups/{id}/permissions");
    let members = format!("/api/admin/groups/{id}/members");
    let grant = |p: &str| Some(json!({ "permissions": [p] }));
    assert_eq!(
        call(GRP, Method::PUT, perms.clone(), grant("admin.accounts")).await,
        403
    );
    assert_eq!(
        call(GRP, Method::PUT, perms.clone(), grant("mail.read")).await,
        403,
        "not held"
    );
    assert_eq!(
        call(
            GRP,
            Method::PUT,
            perms.clone(),
            Some(json!({ "permissions": [] }))
        )
        .await,
        204,
        "control"
    );
    assert_eq!(
        call(SUPER, Method::PUT, perms.clone(), grant("admin.accounts")).await,
        204,
        "control"
    );
    let add = |a: &str| Some(json!({ "address": a }));
    assert_eq!(
        call(GRP, Method::POST, members.clone(), add(GRP)).await,
        403
    );

    // admin.cache, over MCP, adding itself to the admin.accounts group
    let mut mcp = live.mcp(&token[CACHE]).await;
    let refused = mcp
        .tool(
            "add_account_to_group",
            json!({ "group_id": id, "address": CACHE }),
        )
        .await;
    assert!(
        refused.is_err(),
        "admin.cache joined an admin.accounts group"
    );
    // nor does admin.groups get in that way
    let mut mcp = live.mcp(&token[GRP]).await;
    let refused = mcp
        .tool(
            "add_account_to_group",
            json!({ "group_id": id, "address": GRP }),
        )
        .await;
    assert!(
        refused.is_err(),
        "admin.groups joined an admin.accounts group"
    );
    assert_eq!(
        call(SUPER, Method::POST, members, add(PLAIN)).await,
        204,
        "control"
    );

    // creating an account makes no super administrator, whatever is asked
    let created = json!({
        "address": "made@example.test", "display_name": "M", "password": NEW,
        "is_super": true, "permissions": ["admin.accounts"],
    });
    let status = call(
        ACC,
        Method::POST,
        "/api/admin/accounts".into(),
        Some(created),
    )
    .await;
    assert!(
        status == 200 || status == 201 || status == 204,
        "create: {status}"
    );
    let made = live
        .core
        .effective_permissions("made@example.test")
        .await
        .expect("permissions");
    assert!(
        !made.is_super,
        "creating an account made a super administrator"
    );
    assert!(made.permissions.is_empty(), "{:?}", made.permissions);
}
