//! Changing an account — its quota, its password, whether it exists — takes
//! `admin.accounts`, not any administrator's permission. Before, somebody
//! trusted only with the outbound queue could reset anyone's password.

mod live;

use reqwest::Method;
use serde_json::json;

const KEVY: &str = "mem://webapi-account-writes-need-admin-accounts";
const QUEUE: &str = "queue@example.test";
const ACCOUNTS: &str = "accounts@example.test";
const ALICE: &str = "alice@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_admin_accounts_changes_an_account() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [QUEUE, ACCOUNTS, ALICE] {
        live.add_account(who, "pw").await;
    }
    live.grant(QUEUE, "admin.queue").await;
    live.grant(ACCOUNTS, "admin.accounts").await;
    let queue = live.login(QUEUE, "pw").await;
    live.confirm(&queue, "pw").await;
    let accounts = live.login(ACCOUNTS, "pw").await;
    live.confirm(&accounts, "pw").await;

    let alice = "/api/admin/accounts/alice%40example.test";
    let writes = [
        (
            Method::POST,
            format!("{alice}/quota"),
            Some(json!({ "quota_bytes": 1024 })),
        ),
        (
            Method::POST,
            format!("{alice}/password"),
            Some(json!({ "password": "a long new passphrase" })),
        ),
        (
            Method::PUT,
            alice.to_string(),
            Some(json!({ "recovery_email": "r@elsewhere.example" })),
        ),
        (Method::PUT, format!("{alice}/overrides"), Some(json!({}))),
        (
            Method::POST,
            "/api/admin/accounts".to_string(),
            Some(
                json!({ "address": "new@example.test", "display_name": "", "password": "a long new passphrase" }),
            ),
        ),
        (Method::DELETE, alice.to_string(), None),
    ];
    for (method, path, body) in &writes {
        let (status, _) = live.call(method.clone(), path, &queue, body.clone()).await;
        assert_eq!(status, 403, "queue-only: {method} {path}");
    }
    let mut mcp = live.mcp(&queue).await;
    let refused = mcp
        .tool(
            "set_account_password",
            json!({ "address": ALICE, "password": "a long new passphrase" }),
        )
        .await;
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().contains("admin.accounts")),
        "{refused:?}"
    );

    let (status, _) = live
        .call(
            Method::POST,
            &format!("{alice}/quota"),
            &accounts,
            Some(json!({ "quota_bytes": 1024 })),
        )
        .await;
    assert!(
        (200..300).contains(&status),
        "admin.accounts: quota {status}"
    );
}
