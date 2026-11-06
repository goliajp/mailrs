//! The sectioned inbox setting through the web: on until the user turns it
//! off, kept per user, and a body that is not `{enabled: bool}` is refused.

mod live;

use reqwest::Method;
use serde_json::json;

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const PW: &str = "pw";
const PATH: &str = "/api/settings/sectioned-inbox";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_until_turned_off_and_per_user() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_KEVY_URL", &kevy) };
    let live = live::start(&kevy).await;
    live.add_account(ALICE, PW).await;
    live.add_account(BOB, PW).await;
    let alice = live.login(ALICE, PW).await;
    let bob = live.login(BOB, PW).await;

    let (status, got) = live.call(Method::GET, PATH, &alice, None).await;
    assert_eq!(
        (status, got.clone()),
        (200, json!({"enabled": true})),
        "{got}"
    );

    let (status, got) = live
        .call(Method::PUT, PATH, &alice, Some(json!({"enabled": false})))
        .await;
    assert_eq!(
        (status, got.clone()),
        (200, json!({"enabled": false})),
        "{got}"
    );
    let (_, got) = live.call(Method::GET, PATH, &alice, None).await;
    assert_eq!(got, json!({"enabled": false}));
    let (_, got) = live.call(Method::GET, PATH, &bob, None).await;
    assert_eq!(got, json!({"enabled": true}), "alice's choice reached bob");

    for bad in [json!({"enabled": "no"}), json!({})] {
        let (status, _) = live
            .call(Method::PUT, PATH, &alice, Some(bad.clone()))
            .await;
        assert_eq!(status, 422, "{bad} was taken");
    }
    let (_, got) = live.call(Method::GET, PATH, &alice, None).await;
    assert_eq!(
        got,
        json!({"enabled": false}),
        "a refused body changed the setting"
    );
}
