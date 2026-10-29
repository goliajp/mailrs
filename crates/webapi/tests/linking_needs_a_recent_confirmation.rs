//! A signed-in session links a third-party sign-in only after confirming
//! its password recently: a linked identity signs in without the password,
//! so a stolen session must not be able to add one.
//!
//! Driven from the point the provider has vouched for the identity
//! (`external_login::settle`). The token exchange before it reaches the
//! provider through the egress client, which refuses loopback addresses by
//! design, so a provider in a local container cannot be reached from here.

mod live;

use axum::http::{HeaderMap, HeaderValue, header};
use mailrs_core_sidestate::keys::session::{confirmed_key, session_ref};
use mailrs_oauth_client::ExternalIdentity;

const KEVY: &str = "mem://webapi-linking-needs-a-recent-confirmation";
const ALICE: &str = "alice@example.test";

fn identity() -> ExternalIdentity {
    ExternalIdentity {
        issuer: "https://idp.example".into(),
        subject: "subject-1".into(),
        email: Some("alice@idp.example".into()),
        email_verified: true,
        display_name: None,
    }
}

fn kevy() -> kevy_client::Connection {
    kevy_client::Connection::connect(KEVY).expect("kevy")
}

fn linked_to() -> Option<String> {
    mailrs_core_sidestate::families::identity_link::account_for(
        &mut kevy(),
        "https://idp.example",
        "subject-1",
    )
    .expect("lookup")
}

fn location(resp: &axum::response::Response) -> &str {
    resp.headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn linking_waits_for_a_recent_confirmation() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {alice}")).unwrap(),
    );

    // unconfirmed: sent back to settings, nothing linked
    let resp =
        mailrs_webapi::handlers::external_login::settle(&live.state, &headers, "test", identity())
            .await;
    assert_eq!(location(&resp), "/settings?linked=confirm");
    assert_eq!(linked_to(), None, "linked without a confirmation");

    // confirmed: linked to the signed-in account
    live.confirm(&alice, "pw").await;
    let mark = confirmed_key(&session_ref(&alice));
    let before = kevy().ttl_ms(mark.as_bytes()).unwrap();
    let resp =
        mailrs_webapi::handlers::external_login::settle(&live.state, &headers, "test", identity())
            .await;
    assert_eq!(location(&resp), "/settings?linked=1");
    assert_eq!(linked_to().as_deref(), Some(ALICE));

    // and linking did not extend the confirmation
    let after = kevy().ttl_ms(mark.as_bytes()).unwrap();
    assert!(
        after > 0 && after <= before,
        "before {before} ms, after {after} ms"
    );
}
