//! Clearing a password: a super administrator, with a recent confirmation,
//! first sees what clearing would cut off, then clears. The account's
//! sessions end, its signed-in protocol connections are told to close, and
//! its password no longer signs in; its agent keys keep
//! working; the core records who cleared it; the audit log has a row. An
//! administrator who is not super is refused, and so is an address list
//! longer than the limit.

mod live;

use reqwest::Method;
use serde_json::json;

const KEVY: &str = "mem://webapi-password-clear-ends-sign-in";
const ROOT: &str = "root@example.test";
const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";
const PW: &str = "pw";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clearing_ends_sessions_and_password_sign_in() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ROOT, ADMIN, ALICE] {
        live.add_account(who, PW).await;
    }
    live.make_super(ROOT);
    live.grant(ADMIN, "admin.accounts").await;

    let alice = live.login(ALICE, PW).await;
    live.confirm(&alice, PW).await;
    let (status, key) = live
        .call(
            Method::POST,
            "/api/agent/keys",
            &alice,
            Some(json!({ "name": "printer", "scopes": ["mail:read"] })),
        )
        .await;
    assert_eq!(status, 200, "agent key: {key}");
    let key = key["secret"].as_str().expect("secret").to_string();
    let (status, _) = live
        .call(Method::GET, "/api/conversations", &key, None)
        .await;
    assert_eq!(status, 200, "the agent key works before clearing");

    let body = json!({ "addresses": [ALICE, "nobody@example.test"] });
    let admin = live.login(ADMIN, PW).await;
    live.confirm(&admin, PW).await;
    for path in [
        "/api/admin/password-clear/preview",
        "/api/admin/password-clear",
    ] {
        let (status, _) = live
            .call(Method::POST, path, &admin, Some(body.clone()))
            .await;
        assert_eq!(status, 403, "{path} for an administrator who is not super");
    }

    let root = live.login(ROOT, PW).await;
    let (status, got) = live
        .call(
            Method::POST,
            "/api/admin/password-clear/preview",
            &root,
            Some(body.clone()),
        )
        .await;
    assert_eq!(
        (status, got["error"].as_str()),
        (403, Some("confirm_required"))
    );
    live.confirm(&root, PW).await;

    let many: Vec<String> = (0..=100).map(|i| format!("u{i}@example.test")).collect();
    let (status, _) = live
        .call(
            Method::POST,
            "/api/admin/password-clear",
            &root,
            Some(json!({ "addresses": many })),
        )
        .await;
    assert_eq!(status, 413, "more than a hundred addresses");

    let (status, preview) = live
        .call(
            Method::POST,
            "/api/admin/password-clear/preview",
            &root,
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, 200, "preview: {preview}");
    let rows = preview["items"].as_array().expect("items");
    let row = |a: &str| {
        rows.iter()
            .find(|r| r["address"] == a)
            .expect("row")
            .clone()
    };
    let a = row(ALICE);
    assert_eq!(a["exists"], true);
    assert!(a["cleared_at"].is_null());
    assert!(a["sessions"].as_u64().unwrap_or(0) >= 1, "{a}");
    assert_eq!(a["agent_keys"][0]["name"], "printer", "{a}");
    assert_eq!(a["agent_keys"][0]["prefix"], key[..8], "{a}");
    assert_eq!(a["totp"], false);
    assert_eq!(row("nobody@example.test")["exists"], false);
    assert_eq!(preview["issuers_configured"], false);

    // what the protocol listeners hear: each process closes the account's
    // signed-in IMAP, POP3, ManageSieve and SMTP connections on it
    let heard = {
        use mailrs_core_sidestate::keys::signout::CHANNEL;
        let mut sub =
            kevy_client::Subscriber::connect_channels(KEVY, &[CHANNEL]).expect("subscribe");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((_, m)) = sub.recv_message() {
                let _ = tx.send(m);
            }
        });
        rx
    };
    let (status, done) = live
        .call(Method::POST, "/api/admin/password-clear", &root, Some(body))
        .await;
    assert_eq!(status, 200, "clear: {done}");
    let items = done["items"].as_array().expect("items");
    let result = |a: &str| {
        items
            .iter()
            .find(|r| r["address"] == a)
            .expect("item")
            .clone()
    };
    assert_eq!(result(ALICE)["result"], "cleared");
    assert!(result(ALICE)["sessions_revoked"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(result("nobody@example.test")["result"], "unknown");
    let heard = heard
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("no sign-out was published");
    assert_eq!(heard, ALICE.as_bytes(), "the sign-out names the account");

    let (status, _) = live.call(Method::GET, "/api/auth/me", &alice, None).await;
    assert_eq!(status, 401, "alice's session outlived the clearing");
    let res = live
        .http
        .post(format!("{}/api/auth/login", live.base))
        .json(&json!({ "address": ALICE, "password": PW }))
        .send()
        .await
        .expect("login");
    assert_eq!(res.status(), 401, "the old password still signs in");
    let (status, _) = live
        .call(Method::GET, "/api/conversations", &key, None)
        .await;
    assert_eq!(status, 200, "an agent key stops with a password clearing");

    let acct = live
        .core
        .get_account_with_hash(ALICE)
        .await
        .expect("account");
    assert_eq!(acct.password_hash.as_deref(), Some(""));
    assert_eq!(acct.public.password_cleared_by.as_deref(), Some(ROOT));
    assert!(acct.public.password_cleared_at.is_some());

    let (status, log) = live
        .call(
            Method::GET,
            "/api/admin/audit-log?action=account.password_clear",
            &root,
            None,
        )
        .await;
    assert_eq!(status, 200);
    let logged = log["items"].as_array().expect("items");
    assert!(
        logged
            .iter()
            .any(|r| r["actor"] == ROOT && r["target"] == ALICE),
        "no audit row: {log}"
    );
}
