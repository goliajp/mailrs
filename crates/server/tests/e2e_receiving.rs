//! End-to-end receiving regression baseline (PG axis only).
//!
//! Drives a real SMTP DATA delivery over TCP against the *production*
//! `handle_plain_connection` — wired through a genuine `ConnectionContext`
//! by `mailrs_server::test_support::spawn_receiving_server` — and asserts
//! the full local-delivery chain that P1–P3 of the receiver decoupling
//! will refactor:
//!   * maildir file written,
//!   * `messages` row indexed (sender / subject / thread / maildir_id),
//!   * `NewMessage` + `MessageDelivered` events emitted,
//!   * async `post_delivery` side effects (content extraction) applied,
//!   * iTIP invite projected onto `invite_payload` + `InviteReceived`.
//!
//! This is the "behavior unchanged" guardrail: it must stay green on the
//! unchanged tree and after every receiver-decouple step.
//!
//! PG axis only: the spg axis flips `BackendPool` to `SpgPool`, which the
//! `sqlx::PgPool` testcontainer fixture here does not match. The spg axis
//! gets its coverage from the in-process `mailbox` smoke tests.
#![cfg(not(feature = "spg"))]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{connect, read_line, read_multiline, send};
use mailrs_server::{BroadcastEvent, SmtpEvent};
use sqlx::PgPool;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::broadcast::Receiver;

const SCHEMA_SQL: &str = include_str!("../../../scripts/init-schema.sql");
/// Known-good Google iTIP REQUEST, also consumed by the ical / calendar
/// unit tests, so it is guaranteed to parse through the invite path.
const ITIP_REQUEST: &[u8] = include_bytes!("fixtures/itip/google/request.eml");

const USER: &str = "alice@example.com";

/// Spin up a fresh pgvector container with the full `init-schema.sql`
/// applied. The container handle must stay alive for the pool to work.
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

/// Insert the domain + account rows delivery needs. The INBOX mailbox is
/// created by the real delivery path (`ensure_default_mailboxes`).
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

/// Write a message body and the `<CRLF>.<CRLF>` data terminator.
async fn send_dot_terminated(writer: &mut OwnedWriteHalf, body: &[u8]) {
    writer.write_all(body).await.unwrap();
    if !body.ends_with(b"\r\n") {
        writer.write_all(b"\r\n").await.unwrap();
    }
    writer.write_all(b".\r\n").await.unwrap();
}

/// Scan the broadcast stream for the first event matching `pred`,
/// panicking if none arrives within 5 s.
async fn wait_for_event<F>(rx: &mut Receiver<Arc<BroadcastEvent>>, pred: F) -> SmtpEvent
where
    F: Fn(&SmtpEvent) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => {
                if pred(&ev.event) {
                    return ev.event.clone();
                }
            }
            Ok(Err(e)) => panic!("event channel error: {e}"),
            Err(_) => panic!("timed out waiting for expected event"),
        }
    }
}

/// Drive greeting → EHLO → MAIL FROM → RCPT TO → DATA → body, asserting
/// the 250 accept. Leaves the connection open (caller may QUIT).
async fn deliver(port: u16, mail_from: &str, rcpt: &str, body: &[u8]) {
    let (mut reader, mut writer) = connect(port).await;
    let greeting = read_line(&mut reader).await;
    assert!(greeting.starts_with("220 "), "greeting: {greeting}");

    send(&mut writer, "EHLO client.test").await;
    let _ = read_multiline(&mut reader).await;

    send(&mut writer, &format!("MAIL FROM:<{mail_from}>")).await;
    let r = read_line(&mut reader).await;
    assert!(r.starts_with("250"), "MAIL FROM: {r}");

    send(&mut writer, &format!("RCPT TO:<{rcpt}>")).await;
    let r = read_line(&mut reader).await;
    assert!(r.starts_with("250"), "RCPT TO: {r}");

    send(&mut writer, "DATA").await;
    let r = read_line(&mut reader).await;
    assert!(r.starts_with("354"), "DATA: {r}");

    send_dot_terminated(&mut writer, body).await;
    let r = read_line(&mut reader).await;
    assert!(r.starts_with("250"), "data accept: {r}");

    send(&mut writer, "QUIT").await;
}

#[tokio::test]
async fn receiving_normal_mail_indexes_post_delivery_and_events() {
    let (_container, pool) = setup_pg().await;
    seed_account(&pool, USER).await;

    let tmp = tempfile::tempdir().unwrap();
    let maildir_root = tmp.path().to_str().unwrap().to_string();
    let (port, bus) =
        mailrs_server::test_support::spawn_receiving_server(pool.clone(), maildir_root.clone())
            .await;
    let mut rx = bus.subscribe();

    let msg = b"From: Bob Sender <bob@external.com>\r\n\
To: alice@example.com\r\n\
Subject: hello baseline\r\n\
Message-ID: <m-baseline-1@external.com>\r\n\
\r\n\
hello world from the baseline test\r\n";
    deliver(port, "bob@external.com", USER, msg).await;

    // Post-delivery is async (S1.4): NewMessage comes from the consumer
    // task while MessageDelivered comes from the DATA handler, so the two
    // can arrive in either order — gather both without assuming order.
    let mut new_message: Option<SmtpEvent> = None;
    let mut saw_delivered = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while new_message.is_none() || !saw_delivered {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => match &ev.event {
                SmtpEvent::NewMessage { .. } => new_message = Some(ev.event.clone()),
                SmtpEvent::MessageDelivered { .. } => saw_delivered = true,
                _ => {}
            },
            Ok(Err(e)) => panic!("event channel error: {e}"),
            Err(_) => panic!("timed out waiting for NewMessage + MessageDelivered"),
        }
    }
    let SmtpEvent::NewMessage {
        user,
        subject,
        sender,
        thread_id,
        ..
    } = new_message.unwrap()
    else {
        unreachable!()
    };
    assert_eq!(user, USER);
    assert_eq!(subject, "hello baseline");
    assert!(sender.contains("bob@external.com"), "sender: {sender}");
    assert!(!thread_id.is_empty(), "thread resolved");

    // messages row indexed
    let (db_sender, db_subject, db_thread, db_maildir, db_rcpt): (
        String,
        String,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT m.sender, m.subject, m.thread_id, m.maildir_id, m.recipients
         FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE mb.user_address = $1",
    )
    .bind(USER)
    .fetch_one(&pool)
    .await
    .expect("indexed message row");
    assert!(db_sender.contains("bob@external.com"));
    assert_eq!(db_subject, "hello baseline");
    assert!(!db_thread.is_empty(), "thread_id persisted");
    assert!(!db_maildir.is_empty(), "maildir_id persisted");
    assert_eq!(db_rcpt, USER);

    // maildir file written
    let new_dir = format!("{maildir_root}/example.com/alice/new");
    let file_count = std::fs::read_dir(&new_dir).map(|d| d.count()).unwrap_or(0);
    assert!(file_count >= 1, "delivered file in {new_dir}");

    // async post_delivery ran: content extraction populated clean_text
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let clean: Option<String> = sqlx::query_scalar(
            "SELECT m.clean_text FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
             WHERE mb.user_address = $1",
        )
        .bind(USER)
        .fetch_one(&pool)
        .await
        .expect("query clean_text");
        if clean.is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "post_delivery never populated clean_text"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn receiving_itip_request_projects_invite_payload() {
    let (_container, pool) = setup_pg().await;
    seed_account(&pool, USER).await;

    let tmp = tempfile::tempdir().unwrap();
    let maildir_root = tmp.path().to_str().unwrap().to_string();
    let (port, bus) =
        mailrs_server::test_support::spawn_receiving_server(pool.clone(), maildir_root).await;
    let mut rx = bus.subscribe();

    // deliver the known-good iTIP REQUEST to alice (envelope-routed)
    deliver(port, "organizer@gmail.com", USER, ITIP_REQUEST).await;

    let invite = wait_for_event(&mut rx, |e| matches!(e, SmtpEvent::InviteReceived { .. })).await;
    let SmtpEvent::InviteReceived { user, method, .. } = invite else {
        unreachable!()
    };
    assert_eq!(user, USER);
    assert_eq!(method, "REQUEST");

    // invite projected onto the message row
    let (has_payload, invite_method): (bool, Option<String>) = sqlx::query_as(
        "SELECT (m.invite_payload IS NOT NULL), m.invite_method
         FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE mb.user_address = $1",
    )
    .bind(USER)
    .fetch_one(&pool)
    .await
    .expect("indexed invite row");
    assert!(has_payload, "invite_payload projected");
    assert_eq!(invite_method.as_deref(), Some("REQUEST"));
}
