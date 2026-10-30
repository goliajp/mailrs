//! Creating an account over REST takes a password and nothing else. The
//! core also accepts a ready hash, for copying accounts between cores; one
//! sent to webapi is ignored, so an administrator cannot set someone's
//! password to a value they know without going through the password rules.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-account-create-takes-no-hash";
const ADMIN: &str = "admin@example.test";

async fn login_status(live: &live::Live, address: &str, password: &str) -> u16 {
    live.http
        .post(format!("{}/api/auth/login", live.base))
        .json(&serde_json::json!({ "address": address, "password": password }))
        .send()
        .await
        .expect("login")
        .status()
        .as_u16()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hash_sent_with_a_new_account_is_ignored() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ADMIN, "pw").await;
    live.grant(ADMIN, "admin.accounts").await;
    let admin = live.login(ADMIN, "pw").await;
    live.confirm(&admin, "pw").await;

    let known = {
        use argon2::password_hash::{PasswordHasher, SaltString};
        let salt = SaltString::from_b64("c29tZXNhbHRzb21lc2FsdA").expect("salt");
        argon2::Argon2::default()
            .hash_password(b"a-value-the-admin-knows", &salt)
            .expect("hash")
            .to_string()
    };
    for (address, hash) in [
        ("hashed@example.test", known.as_str()),
        ("emptied@example.test", ""),
    ] {
        let (status, body) = live
            .call(
                Method::POST,
                "/api/admin/accounts",
                &admin,
                Some(serde_json::json!({
                    "address": address,
                    "display_name": "",
                    "password": "the-password-sent-1",
                    "password_hash": hash,
                })),
            )
            .await;
        assert_eq!(status, 204, "create {address}: {body}");
        assert_eq!(
            login_status(&live, address, "the-password-sent-1").await,
            200,
            "{address} does not take the password that was sent"
        );
    }
    assert_eq!(
        login_status(&live, "hashed@example.test", "a-value-the-admin-knows").await,
        401,
        "the hash that was sent was stored"
    );
}
