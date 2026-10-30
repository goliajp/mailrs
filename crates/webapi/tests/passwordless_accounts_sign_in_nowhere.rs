//! An account without a usable password signs in nowhere on the web side:
//! not the login form, not the internal verify call, not CalDAV/CardDAV.
//! One kind has an empty hash, which is what clearing a password leaves;
//! the other holds a value an old import stored in clear, which is never
//! compared as text, so typing that value is refused too.

mod live;

use base64::Engine;
use mailrs_core_api::method::admin::SetPasswordRequest;
use reqwest::Method;

const KEVY: &str = "mem://webapi-passwordless-accounts-sign-in-nowhere";
const CLEAR_VALUE: &str = "clear-text-1";
const SERVICE: &str = "service@example.test";

/// Each account is tried at most four times, under the login guard's five,
/// and all of them sixteen, under its twenty from one source.
async fn account(live: &live::Live, address: &str, stored: &str) {
    live.add_account(address, "a-real-password-1").await;
    live.core
        .set_account_password(
            address,
            &SetPasswordRequest {
                password_hash: stored.into(),
            },
        )
        .await
        .expect("store the hash");
}

async fn login(live: &live::Live, address: &str, password: &str) -> u16 {
    live.http
        .post(format!("{}/api/auth/login", live.base))
        .json(&serde_json::json!({ "address": address, "password": password }))
        .send()
        .await
        .expect("login")
        .status()
        .as_u16()
}

async fn dav(live: &live::Live, address: &str, password: &str) -> u16 {
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{address}:{password}"));
    live.http
        .request(
            Method::from_bytes(b"PROPFIND").expect("method"),
            format!("{}/dav/", live.base),
        )
        .header("authorization", format!("Basic {basic}"))
        .header("depth", "0")
        .send()
        .await
        .expect("dav")
        .status()
        .as_u16()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_password_signs_into_an_account_without_a_usable_one() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for address in ["blank", "blank-verify", "blank-dav"] {
        account(&live, &format!("{address}@example.test"), "").await;
    }
    for address in ["clear", "clear-verify", "clear-dav"] {
        account(&live, &format!("{address}@example.test"), CLEAR_VALUE).await;
    }
    live.add_account(SERVICE, "a-real-password-1").await;
    live.grant(SERVICE, "internal.rpc").await;
    let service = live.login(SERVICE, "a-real-password-1").await;

    // every refusal that did not happen, so one run names each way in
    let mut let_in = Vec::new();
    let tries = |kind: &str| {
        [
            (format!("blank{kind}@example.test"), ""),
            (format!("blank{kind}@example.test"), "anything"),
            (format!("clear{kind}@example.test"), CLEAR_VALUE),
            (format!("clear{kind}@example.test"), ""),
        ]
    };
    for (address, password) in tries("") {
        let status = login(&live, &address, password).await;
        if status != 401 {
            let_in.push(format!("login {address} {password:?}: {status}"));
        }
    }
    for (address, password) in tries("-verify") {
        let (status, body) = live
            .call(
                Method::POST,
                "/api/auth/verify",
                &service,
                Some(serde_json::json!({ "address": address, "password": password })),
            )
            .await;
        if (status, &body["valid"]) != (200, &serde_json::json!(false)) {
            let_in.push(format!("verify {address} {password:?}: {status} {body}"));
        }
    }
    for (address, password) in [
        ("blank-dav@example.test", ""),
        ("clear-dav@example.test", CLEAR_VALUE),
    ] {
        let status = dav(&live, address, password).await;
        if status != 401 {
            let_in.push(format!("dav {address} {password:?}: {status}"));
        }
    }
    assert!(let_in.is_empty(), "let in:\n{}", let_in.join("\n"));
}
