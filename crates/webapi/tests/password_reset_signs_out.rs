//! An admin resetting someone's password signs out whoever was signed in
//! with the old one; that person's agent keys keep working. The per-address
//! session index forgets tokens whose session has expired.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-password-reset-signs-out";
const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reset_password_ends_the_old_sessions_only() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ADMIN, ALICE] {
        live.add_account(who, "pw").await;
    }
    live.grant(ADMIN, "admin.accounts").await;
    let admin = live.login(ADMIN, "pw").await;
    let old = live.login(ALICE, "pw").await;
    let (_, created) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &old,
            Some(serde_json::json!({ "name": "ci", "scopes": ["mail:read"] })),
        )
        .await;
    let key = created["secret"].as_str().expect("secret").to_string();

    // an expired session leaves its token behind in the index
    let expired = live.login(ALICE, "pw").await;
    let mut kv = kevy_client::Connection::connect(KEVY).unwrap();
    kv.del(&[mailrs_core_sidestate::keys::session::session_key(&expired).as_bytes()])
        .unwrap();
    let next = live.login(ALICE, "pw").await;
    let indexed = |kv: &mut kevy_client::Connection| -> Vec<String> {
        kv.smembers(mailrs_core_sidestate::keys::session::by_addr_key(ALICE).as_bytes())
            .unwrap()
            .into_iter()
            .map(|m| String::from_utf8(m).unwrap())
            .collect()
    };
    let members = indexed(&mut kv);
    assert!(members.contains(&old) && members.contains(&next));
    assert!(
        !members.contains(&expired),
        "the next login forgets an expired token"
    );

    let (status, _) = live
        .call(
            Method::POST,
            "/api/admin/accounts/alice%40example.test/password",
            &admin,
            Some(serde_json::json!({ "password": "new-pw" })),
        )
        .await;
    assert_eq!(status, 204);

    let me = |token: String| {
        let live = &live;
        async move { live.call(Method::GET, "/api/auth/me", &token, None).await.0 }
    };
    assert_eq!(
        me(old.clone()).await,
        401,
        "signed in with the old password"
    );
    assert_eq!(me(key.clone()).await, 200, "agent keys are separate");
    assert_eq!(me(admin.clone()).await, 200);

    let fresh = live.login(ALICE, "new-pw").await;
    assert_eq!(indexed(&mut kv), [fresh]);
}
