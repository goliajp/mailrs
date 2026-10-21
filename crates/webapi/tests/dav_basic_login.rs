//! Calendar clients log in with HTTP Basic: no credentials gets a 401 with
//! a challenge, the account password gets in, a wrong one does not, and
//! repeated wrong ones lock the client out of that account.

mod live;

use reqwest::StatusCode;

const KEVY: &str = "mem://webapi-dav-basic-login";
const ALICE: &str = "alice@example.test";

async fn propfind(live: &live::Live, auth: Option<(&str, &str)>) -> reqwest::Response {
    let mut req = live
        .http
        .request(
            reqwest::Method::from_bytes(b"PROPFIND").expect("method"),
            format!("{}/dav/", live.base),
        )
        .header("depth", "0");
    if let Some((user, pass)) = auth {
        req = req.basic_auth(user, Some(pass));
    }
    req.send().await.expect("request")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn calendar_clients_log_in_with_basic() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;

    let r = propfind(&live, None).await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let challenge = r.headers().get("www-authenticate").expect("a challenge");
    assert!(challenge.to_str().unwrap().starts_with("Basic "));

    let r = propfind(&live, Some((ALICE, "pw"))).await;
    assert_eq!(r.status(), StatusCode::MULTI_STATUS, "the password gets in");
    assert!(r.text().await.unwrap().contains(ALICE));

    // a session still works, as for the web client
    let bearer = live.login(ALICE, "pw").await;
    let r = live
        .http
        .request(
            reqwest::Method::from_bytes(b"PROPFIND").expect("method"),
            format!("{}/dav/", live.base),
        )
        .header("depth", "0")
        .bearer_auth(&bearer)
        .send()
        .await
        .expect("request");
    assert_eq!(r.status(), StatusCode::MULTI_STATUS);

    for _ in 0..5 {
        let r = propfind(&live, Some((ALICE, "wrong"))).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        assert!(r.headers().contains_key("www-authenticate"));
    }
    let r = propfind(&live, Some((ALICE, "pw"))).await;
    assert_eq!(
        r.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "five wrong passwords lock the client out, even with the right one"
    );
    assert!(r.headers().contains_key("retry-after"));
}
