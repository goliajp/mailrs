//! A real receiver, started the way production starts it: a client in a
//! network on the admin's white list is greeted although the connection
//! rate limit refuses everyone else.
//!
//! The limit is set to zero connections, so every client outside the list
//! is answered 421 before the greeting. The lists load in the background
//! at start, so the first connections may still be refused; a 220 can only
//! come from the list.
//!
//! One test, because the receiver reads its configuration from the
//! environment once per process.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const KEVY: &str = "mem://receiver-admits-listed-networks";

/// The first line the receiver sends, or `None` while it is not listening.
async fn greeting(port: u16) -> Option<String> {
    let stream = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
    let (r, mut w) = stream.into_split();
    let mut line = String::new();
    BufReader::new(r).read_line(&mut line).await.unwrap();
    let _ = w.write_all(b"QUIT\r\n").await;
    Some(line.trim_end().to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_white_network_is_greeted_past_the_rate_limit() {
    let tmp = tempfile::tempdir().unwrap();

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let router =
        mailrs_fastcore::build_router(Arc::new(mailrs_fastcore::FastcoreState::new(mailbox)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let core = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });

    let mut kv = kevy_client::Connection::connect(KEVY).unwrap();
    let entry = serde_json::json!({
        "id": 1, "kind": "cidr", "list": "white", "value": "127.0.0.0/8",
        "note": null, "created_by": "admin@golia.example", "created_at": 1,
    });
    kv.hset(
        mailrs_core_sidestate::keys::admin::GREYLIST_LOCAL_LISTS,
        &[(&b"1"[..], entry.to_string().as_bytes())],
    )
    .unwrap();

    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_CORE_RPC_BASE", &core);
        std::env::set_var("MAILRS_CORE_API_SECRET", "secret");
        std::env::set_var("MAILRS_SPOOL_ROOT", tmp.path().join("spool"));
        std::env::set_var("MAILRS_MAILDIR", tmp.path().join("maildir"));
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "golia.example");
        std::env::set_var("MAILRS_ANTISPAM_ENABLED", "false");
        std::env::set_var("MAILRS_RATE_LIMIT_CAPACITY", "0");
        std::env::set_var("MAILRS_RATE_LIMIT_REFILL", "0");
        std::env::set_var("MAILRS_PORT", "0");
        std::env::set_var("MAILRS_SUBMISSION_PORT", "0");
        std::env::set_var("MAILRS_SMTPS_PORT", "0");
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    // Hold the in-process kevy open before the receiver opens it: its first
    // opens race, and two racing opens of one mem:// name can each get a store.
    let _kevy = kevy_client::Connection::connect(KEVY).unwrap();
    let receiver = mailrs_receiver::start().await;

    let mut seen = Vec::new();
    for _ in 0..200 {
        match greeting(receiver.smtp.port()).await {
            Some(g) if g.starts_with("220") => break,
            Some(g) => {
                assert!(g.starts_with("421"), "neither greeted nor limited: {g}");
                seen.push(g);
            }
            None => {}
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        seen.len() < 200,
        "never greeted, every connection refused: {:?}",
        seen.last()
    );
    for _ in 0..5 {
        let g = greeting(receiver.smtp.port()).await.expect("listening");
        assert!(g.starts_with("220"), "{g}");
    }
}
