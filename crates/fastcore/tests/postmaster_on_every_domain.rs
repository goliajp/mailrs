//! postmaster@ and abuse@ on a local domain with no route of their own
//! reach the same address on the first local domain, so every domain
//! takes mail for postmaster. A domain that routes them itself keeps its
//! own route, and other local parts get no such fallback.

use std::sync::Arc;

use mailrs_alias_store::AliasStore;
use mailrs_core_api::client::Client;
use mailrs_fastcore::FastcoreState;

fn init_env(root: &std::path::Path) {
    // SAFETY: the only test in this binary, before fastcore reads either
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", root);
        std::env::set_var(
            "MAILRS_LOCAL_DOMAINS",
            "primary.example,second.example,own.example",
        );
    }
}

#[tokio::test]
async fn role_addresses_fall_back_to_the_first_local_domain() {
    let tmp = tempfile::tempdir().unwrap();
    init_env(&tmp.path().join("maildir"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    for addr in ["admin@primary.example", "owner@own.example"] {
        let blob = serde_json::json!({ "address": addr, "password_hash": "x" }).to_string();
        mailbox.upsert_account(addr, &blob).unwrap();
    }
    mailbox
        .upsert("postmaster@primary.example", "admin@primary.example")
        .unwrap();
    mailbox
        .upsert("abuse@primary.example", "admin@primary.example")
        .unwrap();
    mailbox
        .upsert("postmaster@own.example", "owner@own.example")
        .unwrap();

    let router = mailrs_fastcore::build_router(Arc::new(FastcoreState::new(mailbox)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    let core = Client::new(base, "secret");

    for addr in [
        "postmaster@second.example",
        "abuse@second.example",
        "postmaster@own.example",
        "abuse@own.example",
    ] {
        assert!(core.recipient_deliverable(addr).await.unwrap(), "{addr}");
    }
    for addr in ["info@second.example", "postmaster@elsewhere.example"] {
        assert!(!core.recipient_deliverable(addr).await.unwrap(), "{addr}");
    }
}
