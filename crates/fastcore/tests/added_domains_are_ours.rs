//! A domain added on the admin Domains page counts as ours in both
//! processes that decide it: the receiver, which lists the domain store
//! over core RPC, and fastcore's spool drain, which reads it directly.
//! Before this both read `MAILRS_LOCAL_DOMAINS` alone.

use std::sync::Arc;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_fastcore::FastcoreState;
use mailrs_receiver::runtime_settings::{LocalDomains, refresh_local_domains};

fn fastcore() -> Arc<FastcoreState> {
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    Arc::new(FastcoreState::new(
        mailrs_mailbox_kevy::KevyMailboxStore::new(store),
    ))
}

async fn serve(state: Arc<FastcoreState>) -> String {
    let router = mailrs_fastcore::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{addr}")
}

#[tokio::test]
async fn the_receiver_takes_a_domain_added_through_the_core_on_refresh() {
    let base = serve(fastcore()).await;
    let core = mailrs_core_api::client::Client::new(base, "secret");
    let domains = LocalDomains::new(vec!["golia.example".into()]);

    assert!(!refresh_local_domains(&domains, &core).await.expect("list"));
    assert_eq!(*domains.current(), ["golia.example"]);

    core.add_domain("Added.Example").await.expect("add domain");
    assert!(refresh_local_domains(&domains, &core).await.expect("list"));
    assert_eq!(*domains.current(), ["golia.example", "added.example"]);
}

/// A core that cannot be reached leaves the set as it was: clearing it
/// would make an empty list, which means "every domain is ours".
#[tokio::test]
async fn a_failed_fetch_keeps_the_previous_set() {
    let base = serve(fastcore()).await;
    let core = mailrs_core_api::client::Client::new(base, "secret");
    core.add_domain("added.example").await.expect("add domain");
    let domains = LocalDomains::new(Vec::new());
    refresh_local_domains(&domains, &core).await.expect("list");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let dead = format!("http://{}", listener.local_addr().expect("addr"));
    drop(listener);
    let dead = mailrs_core_api::client::Client::new(dead, "secret");
    assert!(refresh_local_domains(&domains, &dead).await.is_err());
    assert_eq!(*domains.current(), ["added.example"]);
}

fn submission_to(spool: &std::path::Path, rcpt: &str) {
    let env = SpoolEnvelope {
        reverse_path: "alice@golia.example".into(),
        forward_paths: vec![rcpt.into()],
        is_authenticated: true,
        auth_user: None,
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    };
    std::fs::create_dir_all(spool).unwrap();
    let blob = encode_spool_blob(&env, b"Subject: hi\r\n\r\nhello");
    std::fs::write(spool.join("1700000000.M1P1.sub"), blob).unwrap();
}

fn delivered(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir.join("new")).unwrap().count()
}

/// An authenticated submission to a domain on the page is delivered
/// here, not handed to the outbound queue.
#[tokio::test]
async fn the_drain_delivers_to_a_store_domain_instead_of_relaying_it() {
    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    let bob = maildir.join("added.example/bob");
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(bob.join(sub)).unwrap();
    }
    let root = maildir.to_str().unwrap();

    // not on the page: an outward relay, which fails here for want of a
    // queue, so nothing reaches bob
    let without = fastcore();
    let spool = tmp.path().join("spool-without");
    submission_to(&spool, "bob@added.example");
    mailrs_fastcore::spool_drain::drain_once(&spool, root, &without);
    assert_eq!(delivered(&bob), 0);

    let with = fastcore();
    with.mailbox.upsert_domain("added.example", 1).unwrap();
    let spool = tmp.path().join("spool-with");
    submission_to(&spool, "bob@added.example");
    let (count, _) = mailrs_fastcore::spool_drain::drain_once(&spool, root, &with);
    assert_eq!(count, 1);
    assert_eq!(delivered(&bob), 1);
}
