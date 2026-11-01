//! A webhook subscription names an event that is raised: an unknown name is
//! refused instead of stored as a subscription that would never fire, and
//! the old name `new_message` is still accepted.

mod live;

use reqwest::Method;
use serde_json::json;

const KEVY: &str = "mem://webapi-webhook-event-types";
const ALICE: &str = "alice@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_events_that_are_raised_can_be_subscribed_to() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.confirm(&alice, "pw").await;

    let create = |kind: &'static str| {
        let live = &live;
        let alice = alice.clone();
        async move {
            live.call(
                Method::POST,
                "/api/agent/webhooks",
                &alice,
                Some(json!({ "url": "https://hooks.example/in", "event_type": kind })),
            )
            .await
            .0
        }
    };
    assert_eq!(create("mail.new").await, 400, "never raised");
    assert_eq!(create("").await, 400);
    assert_eq!(create("thread.archived").await, 200);
    assert_eq!(
        create("new_message").await,
        200,
        "the old name of mail.received"
    );

    let (status, listed) = live
        .call(Method::GET, "/api/agent/webhooks", &alice, None)
        .await;
    assert_eq!(status, 200);
    let kinds: Vec<&str> = listed["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|w| w["event_type"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(kinds.len(), 2, "{listed}");
    assert!(kinds.contains(&"thread.archived"));
}
