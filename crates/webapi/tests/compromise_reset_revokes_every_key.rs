//! A reset after a compromise revokes every agent key the account has,
//! once the administrator has seen them: the reset names the keys it was
//! shown, and goes ahead only while they are all of them. Each revoked key
//! is audited. Keys record when they were last used, to the hour.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-compromise-reset-revokes-every-key";
const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";
const NEW: &str = "a much longer new passphrase";
const KEYS: &str = "/api/admin/accounts/alice%40example.test/agent-keys";
const RESET: &str = "/api/admin/accounts/alice%40example.test/password";

fn ids(v: &Value) -> Vec<i64> {
    let mut ids: Vec<i64> = v
        .as_array()
        .expect("a list")
        .iter()
        .map(|k| k["id"].as_i64().expect("id"))
        .collect();
    ids.sort_unstable();
    ids
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_compromise_reset_revokes_every_key_it_was_shown() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ADMIN, ALICE] {
        live.add_account(who, "pw").await;
    }
    live.grant(ADMIN, "admin.accounts").await;
    let admin = live.login(ADMIN, "pw").await;
    live.confirm(&admin, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.confirm(&alice, "pw").await;
    let mut secrets = Vec::new();
    for name in ["used", "idle"] {
        let body = json!({ "name": name, "scopes": ["mail:read"] });
        let (_, created) = live
            .call(Method::POST, "/api/agent/keys", &alice, Some(body))
            .await;
        secrets.push(created["secret"].as_str().expect("secret").to_string());
    }
    let me = |token: String| {
        let live = &live;
        async move { live.call(Method::GET, "/api/auth/me", &token, None).await.0 }
    };
    assert_eq!(me(secrets[0].clone()).await, 200);

    // the list says when each key was last used, and null when it was not
    let (status, listed) = live.call(Method::GET, KEYS, &admin, None).await;
    assert_eq!(status, 200, "{listed}");
    assert!(listed["last_use_recorded_since"].is_string());
    let items = listed["items"].as_array().expect("items").clone();
    assert_eq!(items.len(), 2);
    let by_name = |n: &str| items.iter().find(|k| k["name"] == n).expect(n).clone();
    let now = chrono::Utc::now().timestamp();
    let used = by_name("used")["last_used_at"].as_i64().expect("recorded");
    assert!((now - used).abs() < 60, "{used} is not now");
    assert!(by_name("idle")["last_used_at"].is_null());
    assert!(by_name("idle")["created_at"].as_i64().is_some());
    let shown = ids(&listed["items"]);

    // a list that is not every key is refused, with the keys as they are
    let partial = json!({ "password": NEW, "compromised": { "revoke_key_ids": [shown[0]] } });
    let (status, got) = live.call(Method::POST, RESET, &admin, Some(partial)).await;
    assert_eq!(
        (status, got["error"].as_str()),
        (409, Some("key_list_changed"))
    );
    assert_eq!(ids(&got["keys"]), shown);
    assert_eq!(
        me(secrets[1].clone()).await,
        200,
        "a refused reset revoked a key"
    );
    assert_eq!(
        me(alice.clone()).await,
        200,
        "a refused reset signed alice out"
    );

    let all = json!({ "password": NEW, "compromised": { "revoke_key_ids": shown } });
    let (status, got) = live.call(Method::POST, RESET, &admin, Some(all)).await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(got["agent_keys_revoked"], 2);
    assert!(got["sessions_revoked"].as_u64().expect("count") >= 1);
    for secret in &secrets {
        assert_eq!(me(secret.clone()).await, 401, "a key survived");
    }
    assert_eq!(me(alice.clone()).await, 401);
    let (_, after) = live.call(Method::GET, KEYS, &admin, None).await;
    assert_eq!(after["items"], json!([]));
    let mut kevy = kevy_client::Connection::connect(KEVY).unwrap();
    let last_used = kevy
        .hgetall(
            mailrs_core_sidestate::keys::credentials::agent_keys_last_used_key(ALICE).as_bytes(),
        )
        .unwrap();
    assert!(last_used.is_empty(), "revoked keys left their last use");

    let audit: Vec<Value> = kevy
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice(r).ok())
        .collect();
    let revoked = audit
        .iter()
        .filter(|r| r["action"] == "agent_key.revoke" && r["target"] == ALICE)
        .count();
    assert_eq!(revoked, 2, "each key is audited");
    assert!(
        audit.iter().any(|r| r["action"] == "account.password_reset"
            && r["target"] == ALICE
            && r["detail"]
                .as_str()
                .is_some_and(|d| d.contains("compromise, keys=2"))),
        "the reset is audited as a compromise"
    );
    assert!(
        !audit
            .iter()
            .any(|r| secrets.iter().any(|s| r.to_string().contains(s.as_str()))),
        "a key reached the audit log"
    );
    live.login(ALICE, NEW).await;
}
