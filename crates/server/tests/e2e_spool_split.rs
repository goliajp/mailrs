//! End-to-end split-delivery test (PG axis only).
//!
//! Drives a real SMTP DATA delivery against a **receiver-mode**
//! `ConnectionContext` (spool_sink set) wired by
//! `test_support::spawn_split_receiving_server`, which also runs the core
//! spool consumer subscribing to the same event bus. So the message flows
//! SMTP → spool file → SpoolDelivered → consume → deliver → index — the full
//! receiver/core split — and we assert the **same** final state the monolith
//! `e2e_receiving` asserts: a `messages` row + a `NewMessage` event. This
//! proves the split delivers identically to the inline path.
#![cfg(not(feature = "spg"))]

mod common;

use std::time::{Duration, Instant};

use common::{connect, read_line, read_multiline, send};
use mailrs_server::SmtpEvent;
use sqlx::PgPool;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;

const SCHEMA_SQL: &str = include_str!("../../../scripts/init-schema.sql");
const USER: &str = "alice@example.com";

async fn setup_pg() -> (ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("pgvector/pgvector", "pg18")
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_exposed_port(5432.tcp())
        .with_env_var("POSTGRES_PASSWORD", "test")
        .with_env_var("POSTGRES_DB", "mailrs_test")
        .with_env_var("POSTGRES_USER", "postgres")
        .start()
        .await
        .expect("start pgvector container");
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");
    let url = format!("postgres://postgres:test@{host}:{port}/mailrs_test");
    let deadline = Instant::now() + Duration::from_secs(10);
    let pool = loop {
        match PgPool::connect(&url).await {
            Ok(p) => break p,
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => panic!("pg pool never came up: {e}"),
        }
    };
    sqlx::raw_sql(SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply init-schema.sql");
    (container, pool)
}

async fn seed_account(pool: &PgPool, user: &str) {
    let domain = user
        .split_once('@')
        .map(|(_, d)| d)
        .unwrap_or("example.com");
    sqlx::query("INSERT INTO domains (name) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(domain)
        .execute(pool)
        .await
        .expect("seed domain");
    sqlx::query("INSERT INTO accounts (address, domain) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(user)
        .bind(domain)
        .execute(pool)
        .await
        .expect("seed account");
}

async fn send_dot_terminated(writer: &mut OwnedWriteHalf, body: &[u8]) {
    writer.write_all(body).await.unwrap();
    if !body.ends_with(b"\r\n") {
        writer.write_all(b"\r\n").await.unwrap();
    }
    writer.write_all(b".\r\n").await.unwrap();
}

async fn deliver(port: u16, mail_from: &str, rcpt: &str, body: &[u8]) {
    let (mut reader, mut writer) = connect(port).await;
    let greeting = read_line(&mut reader).await;
    assert!(greeting.starts_with("220 "), "greeting: {greeting}");
    send(&mut writer, "EHLO client.test").await;
    let _ = read_multiline(&mut reader).await;
    send(&mut writer, &format!("MAIL FROM:<{mail_from}>")).await;
    assert!(read_line(&mut reader).await.starts_with("250"));
    send(&mut writer, &format!("RCPT TO:<{rcpt}>")).await;
    assert!(read_line(&mut reader).await.starts_with("250"));
    send(&mut writer, "DATA").await;
    assert!(read_line(&mut reader).await.starts_with("354"));
    send_dot_terminated(&mut writer, body).await;
    let r = read_line(&mut reader).await;
    assert!(r.starts_with("250"), "data accept (receiver spooled): {r}");
    send(&mut writer, "QUIT").await;
}

#[tokio::test]
async fn split_delivery_spools_then_core_consumes_and_indexes() {
    let (_container, pool) = setup_pg().await;
    seed_account(&pool, USER).await;

    let maildir_tmp = tempfile::tempdir().unwrap();
    let spool_tmp = tempfile::tempdir().unwrap();
    let maildir_root = maildir_tmp.path().to_str().unwrap().to_string();
    let spool_root = spool_tmp.path().to_str().unwrap().to_string();

    let (port, bus) = mailrs_server::test_support::spawn_split_receiving_server(
        pool.clone(),
        maildir_root,
        spool_root,
    )
    .await;
    let mut rx = bus.subscribe();

    let msg = b"From: Bob Sender <bob@external.com>\r\n\
To: alice@example.com\r\n\
Subject: hello split\r\n\
Message-ID: <m-split-1@external.com>\r\n\
\r\n\
hello world from the split test\r\n";
    deliver(port, "bob@external.com", USER, msg).await;

    // the core consumer indexes asynchronously after fetching the spool file;
    // wait for the NewMessage it emits (proves spool → consume → deliver →
    // index ran), tolerating a wider deadline than the monolith path.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut new_message: Option<SmtpEvent> = None;
    while new_message.is_none() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => {
                if let SmtpEvent::NewMessage { .. } = &ev.event {
                    new_message = Some(ev.event.clone());
                }
            }
            Ok(Err(e)) => panic!("event channel error: {e}"),
            Err(_) => panic!("timed out waiting for NewMessage from the spool consumer"),
        }
    }

    if let Some(SmtpEvent::NewMessage { user, subject, .. }) = &new_message {
        assert_eq!(user, USER, "delivered to the resolved local recipient");
        assert_eq!(subject, "hello split", "subject carried through the spool");
    }

    // the messages row exists — same final state as the monolith path.
    let row: (String, String) = sqlx::query_as(
        "SELECT m.sender, m.subject FROM messages m
         JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE mb.user_address = $1 AND m.subject = $2",
    )
    .bind(USER)
    .bind("hello split")
    .fetch_one(&pool)
    .await
    .expect("messages row indexed from the spool-consumed delivery");
    assert!(row.0.contains("bob"), "sender preserved: {}", row.0);
}
