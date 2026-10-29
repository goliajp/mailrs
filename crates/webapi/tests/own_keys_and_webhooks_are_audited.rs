//! Making and removing your own agent keys and webhooks leaves an audit
//! row. Removing one asks for no password, so the record is what shows
//! that an alerting key was deleted, and by whom. The rows name the key by
//! its listed prefix and the webhook by id: never a secret, never a URL,
//! which can carry the receiver's token.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-own-keys-and-webhooks-are-audited";
const ALICE: &str = "alice@example.test";

fn audit_rows(action: &str) -> Vec<Value> {
    let mut c = kevy_client::Connection::connect(KEVY).unwrap();
    c.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<Value>(r).unwrap())
        .filter(|r| r["action"] == action)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn own_keys_and_webhooks_leave_an_audit_row() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.confirm(&alice, "pw").await;

    let (status, key) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &alice,
            Some(json!({ "name": "alerts", "scopes": ["mail:read"] })),
        )
        .await;
    assert_eq!(status, 200, "{key}");
    let (id, secret) = (key["id"].as_i64().unwrap(), key["secret"].as_str().unwrap());
    let (status, hook) = live
        .call(
            Method::POST,
            "/api/agent/webhooks",
            &alice,
            Some(json!({ "url": "https://hooks.example/in?token=SECRET-IN-URL", "event_type": "new_mail" })),
        )
        .await;
    assert!((200..300).contains(&status), "{hook}");
    let hook_id = hook["id"].as_i64().unwrap();

    let (status, _) = live
        .call(
            Method::DELETE,
            &format!("/api/agent/keys/{id}"),
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 204);
    let (status, _) = live
        .call(
            Method::DELETE,
            &format!("/api/agent/webhooks/{hook_id}"),
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 204);

    for (action, detail) in [
        (
            "agent_key.create",
            format!("id={id} prefix={}", &secret[..8]),
        ),
        (
            "agent_key.revoke",
            format!("id={id} prefix={}", &secret[..8]),
        ),
        ("webhook.create", format!("id={hook_id}")),
        ("webhook.delete", format!("id={hook_id}")),
    ] {
        let rows = audit_rows(action);
        assert_eq!(rows.len(), 1, "{action}: {rows:?}");
        assert_eq!(rows[0]["actor"], ALICE, "{action}");
        let got = rows[0]["detail"].as_str().unwrap();
        assert!(got.starts_with(&detail), "{action}: {got}");
        assert!(
            !got.contains(secret) && !got.contains("SECRET-IN-URL"),
            "{action}: {got}"
        );
    }
}
