//! SMTP submission authenticates against the accounts fastcore holds.
//!
//! The receiver is a separate process and cannot open fastcore's
//! embedded kevy, so for months it checked passwords against users.toml
//! alone: every real account got 535 on 587 / 465 while the same
//! password signed in to the web UI and to IMAP. This runs the receiver's
//! lookup against a real fastcore over HTTP.

use std::sync::Arc;

use mailrs_core::users::UserStore;
use mailrs_receiver::{CoreCredentialStore, CredentialStore};

async fn serve_fastcore() -> String {
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    let router = mailrs_fastcore::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{addr}")
}

#[tokio::test]
async fn an_account_added_through_the_core_authenticates_for_submission() {
    let base = serve_fastcore().await;
    let core = mailrs_core_api::client::Client::new(base.clone(), "secret");
    core.add_account(&mailrs_core_api::method::admin::AddAccountRequest {
        address: "alice@example.com".into(),
        display_name: "Alice".into(),
        password: "correct horse".into(),
        password_hash: None,
    })
    .await
    .expect("add account");

    let creds = CoreCredentialStore::new(base, "secret".into());
    let hash = creds
        .password_hash("alice@example.com")
        .await
        .expect("lookup")
        .expect("the account exists");
    assert!(hash.starts_with("$argon2"), "an argon2 hash: {hash}");
    assert!(UserStore::verify_hash("correct horse", &hash));
    assert!(!UserStore::verify_hash("wrong", &hash));
}

/// "No such account" and "could not ask" must stay apart: the first is
/// an ordinary 535, the second is logged as a failed lookup.
#[tokio::test]
async fn an_unknown_address_is_none_and_an_unreachable_core_is_an_error() {
    let base = serve_fastcore().await;
    let creds = CoreCredentialStore::new(base, "secret".into());
    assert!(
        creds
            .password_hash("nobody@example.com")
            .await
            .expect("lookup")
            .is_none()
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let dead = format!("http://{}", listener.local_addr().expect("addr"));
    drop(listener);
    let creds = CoreCredentialStore::new(dead, "secret".into());
    assert!(creds.password_hash("alice@example.com").await.is_err());
}
