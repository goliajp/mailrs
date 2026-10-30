//! An administrator resetting a password over MCP signs the account out,
//! the same as over REST: no session of it is left. Its agent keys stay.

mod live;

use reqwest::Method;
use serde_json::json;

const KEVY: &str = "mem://webapi-mcp-password-reset-signs-out";
const ADMIN: &str = "admin@example.test";
const BOB: &str = "bob@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reset_over_mcp_leaves_no_session() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ADMIN, BOB] {
        live.add_account(who, "pw").await;
    }
    live.grant(ADMIN, "admin.accounts").await;
    let admin = live.login(ADMIN, "pw").await;
    live.confirm(&admin, "pw").await;
    let bob = live.login(BOB, "pw").await;
    live.confirm(&bob, "pw").await;
    let (_, created) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &bob,
            Some(json!({ "name": "ci", "scopes": ["mail:read"] })),
        )
        .await;
    let key = created["secret"].as_str().expect("secret").to_string();
    let mut kevy = kevy_client::Connection::connect(KEVY).unwrap();
    let sessions = |kevy: &mut kevy_client::Connection| {
        kevy.scard(mailrs_core_sidestate::keys::session::by_addr_key(BOB).as_bytes())
            .unwrap()
    };
    assert_eq!(sessions(&mut kevy), 1);

    let mut mcp = live.mcp(&admin).await;
    let got = mcp
        .tool(
            "set_account_password",
            json!({ "address": BOB, "password": "a much longer new passphrase" }),
        )
        .await
        .expect("reset");
    assert_eq!(got["ok"], true);
    assert_eq!(got["agent_keys_revoked"], 0);

    assert_eq!(sessions(&mut kevy), 0, "a session survived the reset");
    let me = |token: String| {
        let live = &live;
        async move { live.call(Method::GET, "/api/auth/me", &token, None).await.0 }
    };
    assert_eq!(me(bob).await, 401);
    assert_eq!(me(key).await, 200, "a routine reset keeps the keys");
}
