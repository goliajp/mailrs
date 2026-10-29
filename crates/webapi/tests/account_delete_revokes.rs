//! Deleting an account ends what it was signed in with: its sessions and
//! agent keys stop authenticating, through REST and MCP alike, and nobody
//! else's do.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-account-delete-revokes";
const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_account_keeps_no_session_and_no_agent_key() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ADMIN, ALICE, BOB] {
        live.add_account(who, "pw").await;
    }
    live.grant(ADMIN, "admin.accounts").await;
    let admin = live.login(ADMIN, "pw").await;
    live.confirm(&admin, "pw").await;

    let alice = live.login(ALICE, "pw").await;
    live.confirm(&alice, "pw").await;
    let alice_second = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;
    live.confirm(&bob, "pw").await;
    let (status, created) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &alice,
            Some(serde_json::json!({ "name": "ci", "scopes": ["mail:read"] })),
        )
        .await;
    assert_eq!(status, 200);
    let alice_key = created["secret"].as_str().expect("secret").to_string();
    let (_, created) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &bob,
            Some(serde_json::json!({ "name": "ci", "scopes": ["mail:read"] })),
        )
        .await;
    let bob_key = created["secret"].as_str().expect("secret").to_string();

    let me = |token: String| {
        let live = &live;
        async move { live.call(Method::GET, "/api/auth/me", &token, None).await.0 }
    };
    for t in [&alice, &alice_second, &alice_key, &bob, &bob_key] {
        assert_eq!(me(t.clone()).await, 200, "works before the delete");
    }

    let (status, _) = live
        .call(
            Method::DELETE,
            "/api/admin/accounts/alice%40example.test",
            &admin,
            None,
        )
        .await;
    assert_eq!(status, 204);

    for t in [&alice, &alice_second, &alice_key] {
        assert_eq!(me(t.clone()).await, 401, "alice's credentials are gone");
    }
    let mut mcp = live.mcp(&alice_key).await;
    let refused = mcp.tool("get_my_permissions", serde_json::json!({})).await;
    assert!(refused.is_err(), "MCP sees no caller: {refused:?}");
    for t in [&bob, &bob_key] {
        assert_eq!(me(t.clone()).await, 200, "bob's are untouched");
    }

    // the MCP tool revokes the same way
    let mut mcp = live.mcp(&admin).await;
    mcp.tool("remove_account", serde_json::json!({ "address": BOB }))
        .await
        .expect("remove_account");
    for t in [&bob, &bob_key] {
        assert_eq!(me(t.clone()).await, 401, "bob's credentials are gone");
    }
    assert_eq!(me(admin.clone()).await, 200);
}
