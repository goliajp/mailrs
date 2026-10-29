//! A credential in the URL is not a credential. A token in a query string
//! ends up in proxy logs, browser history and `Referer` headers, so the
//! server only reads it from the `Authorization` header or the cookie.

mod live;

const KEVY: &str = "mem://webapi-tokens-never-in-the-url";
const ALICE: &str = "alice@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_token_in_the_query_signs_nobody_in() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let token = live.login(ALICE, "pw").await;

    let by_query = live
        .http
        .get(format!("{}/api/auth/me?token={token}", live.base))
        .send()
        .await
        .expect("request");
    assert_eq!(by_query.status(), 401);
    let (by_header, _) = live
        .call(reqwest::Method::GET, "/api/auth/me", &token, None)
        .await;
    assert_eq!(by_header, 200);

    for path in ["/api/events", "/api/events/smtp"] {
        let url = format!(
            "{}{path}?token={token}",
            live.base.replacen("http", "ws", 1)
        );
        let refused = tokio_tungstenite::connect_async(url).await;
        assert!(
            matches!(
                refused,
                Err(tokio_tungstenite::tungstenite::Error::Http(ref r)) if r.status() == 401
            ),
            "{path} upgraded on a query token"
        );
    }
}
