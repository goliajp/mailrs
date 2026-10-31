//! App passwords end to end through webapi: made only after a recent
//! password confirmation and shown once; a calendar client signs in with
//! one; an account set to app passwords only refuses its own password there
//! and says where to make one; the administrator's count tells the two
//! apart; a revoked one stops working.

mod live;

use reqwest::{Method, StatusCode};
use serde_json::json;

const KEVY: &str = "mem://webapi-app-passwords";
const ADMIN: &str = "admin@example.test";
const BOB: &str = "bob@example.test";

async fn propfind(live: &live::Live, user: &str, pass: &str) -> reqwest::Response {
    live.http
        .request(
            Method::from_bytes(b"PROPFIND").expect("method"),
            format!("{}/dav/", live.base),
        )
        .header("depth", "0")
        .basic_auth(user, Some(pass))
        .send()
        .await
        .expect("request")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_app_password_signs_a_mail_app_in_and_can_be_required() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_HOSTNAME", "mail.example.test");
    }
    let live = live::start(KEVY).await;
    for who in [ADMIN, BOB] {
        live.add_account(who, "pw").await;
    }
    live.make_super(ADMIN);
    let admin = live.login(ADMIN, "pw").await;
    let bob = live.login(BOB, "pw").await;

    // made only after confirming the password
    let ask = json!({ "name": "Thunderbird" });
    let (status, got) = live
        .call(
            Method::POST,
            "/api/auth/app-passwords",
            &bob,
            Some(ask.clone()),
        )
        .await;
    assert_eq!(
        (status, got["error"].as_str()),
        (403, Some("confirm_required"))
    );
    live.confirm(&bob, "pw").await;
    let (status, made) = live
        .call(Method::POST, "/api/auth/app-passwords", &bob, Some(ask))
        .await;
    assert_eq!(status, 201, "{made}");
    let password = made["password"].as_str().expect("the password").to_string();
    assert_eq!(password.len(), 16);
    let (status, listed) = live
        .call(Method::GET, "/api/auth/app-passwords", &bob, None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    assert_eq!(listed[0]["name"], "Thunderbird");
    assert!(
        !listed.to_string().contains(&password),
        "listed with its password"
    );

    // both work while the account allows its own password
    assert_eq!(
        propfind(&live, BOB, &password).await.status(),
        StatusCode::MULTI_STATUS
    );
    assert_eq!(
        propfind(&live, BOB, "pw").await.status(),
        StatusCode::MULTI_STATUS
    );

    // the administrator sees which was used
    let (status, usage) = live
        .call(Method::GET, "/api/admin/protocol-logins", &admin, None)
        .await;
    assert_eq!(status, 200, "{usage}");
    let row = usage["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|r| r["account"] == BOB && r["channel"] == "dav")
        .expect("bob's DAV sign-ins")
        .clone();
    assert!(row["app_password_logins"].as_u64() >= Some(1));
    assert!(row["password_logins"].as_u64() >= Some(1));
    assert_eq!(usage["default"], "allow");
    assert_eq!(usage["policies"][BOB], "inherit");

    // set to app passwords only: the account password is refused there, and
    // the reply says where to make one; the app password still works
    let path = "/api/admin/accounts/bob%40example.test/protocol-password";
    let (status, _) = live
        .call(
            Method::PUT,
            path,
            &admin,
            Some(json!({ "policy": "app-only" })),
        )
        .await;
    assert_eq!(status, 204);
    let (status, _) = live
        .call(Method::PUT, path, &admin, Some(json!({ "policy": "off" })))
        .await;
    assert_eq!(status, 400);
    let refused = propfind(&live, BOB, "pw").await;
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    assert!(
        refused
            .text()
            .await
            .unwrap()
            .contains("https://mail.example.test/settings?tab=security")
    );
    assert_eq!(
        propfind(&live, BOB, &password).await.status(),
        StatusCode::MULTI_STATUS
    );
    // the web client is not a mail app: its own password still signs in
    live.login(BOB, "pw").await;

    // revoked, it stops working
    let id = listed[0]["id"].as_str().expect("id");
    let (status, _) = live
        .call(
            Method::DELETE,
            &format!("/api/auth/app-passwords/{id}"),
            &bob,
            None,
        )
        .await;
    assert_eq!(status, 204);
    assert_eq!(
        propfind(&live, BOB, &password).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let (status, _) = live
        .call(
            Method::DELETE,
            &format!("/api/auth/app-passwords/{id}"),
            &bob,
            None,
        )
        .await;
    assert_eq!(status, 404);
}
