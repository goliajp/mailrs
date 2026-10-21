//! The drain's lifecycle and routing over this store.

use super::Fixture;

const BODY: &[u8] = b"From: Carol <carol@outside.example>\r\n\
To: alice@example.com\r\n\
Subject: drained\r\n\
Message-ID: <drained-1@outside.example>\r\n\
\r\n\
hello\r\n";

/// A file in `cur` is drained like one in `new`, and an alias reaches its
/// target's mailbox.
#[tokio::test(flavor = "multi_thread")]
async fn an_alias_in_cur_reaches_its_target() {
    let f = Fixture::new().await;
    f.account("alice@example.com").await;
    f.alias("contact@example.com", "alice@example.com").await;
    let file = f.spool(
        "cur",
        "1790000000.M1P1Q1.host",
        &["Contact@Example.com"],
        BODY,
    );

    assert_eq!(f.tick().await, 1);
    assert!(!file.exists(), "a delivered file stays in the spool");
    assert!(f.delivered("alice@example.com", "1790000000.M1P1Q1.host"));
    assert_eq!(
        f.row("alice@example.com", "drained").await.as_deref(),
        Some("1790000000.M1P1Q1.host")
    );
}

/// Nobody takes mail at an address with no mailbox, alias or group: the
/// file stays for a human, and no mailbox is made for it.
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_recipient_leaves_the_file_and_makes_no_mailbox() {
    let f = Fixture::new().await;
    f.account("alice@example.com").await;
    let file = f.spool(
        "new",
        "1790000001.M1P1Q1.host",
        &["ghost@example.com"],
        BODY,
    );

    assert_eq!(f.tick().await, 0);
    assert!(file.exists(), "an undeliverable file was removed");
    assert!(!f.maildir.path().join("example.com").join("ghost").exists());
}

/// A file that does not decode is kept, never deleted.
#[tokio::test(flavor = "multi_thread")]
async fn an_undecodable_file_is_kept() {
    let f = Fixture::new().await;
    let file = f.spool_raw("new", "garbage", b"not a spool envelope");
    assert_eq!(f.tick().await, 0);
    assert!(file.exists());
}

/// A message handed over on the bounce queue — a DSN from the sender, a
/// simulated reply — lands in the local sender's INBOX here too.
#[tokio::test(flavor = "multi_thread")]
async fn a_queued_bounce_reaches_the_senders_inbox() {
    const NET: &str = "mem://pg-intake-bounce-queue";
    let mut keep = kevy_client::Connection::connect(NET).expect("kevy");
    let f = Fixture::with_net(Some(NET)).await;
    f.account("alice@example.com").await;
    let dsn = b"From: Mail Delivery System <MAILER-DAEMON@example.com>\r\n\
To: <alice@example.com>\r\n\
Subject: Undelivered Mail Returned to Sender\r\n\
Message-ID: <dsn-1@example.com>\r\n\
\r\n\
gone@remote.example could not be reached\r\n";
    mailrs_intake::notice::enqueue_notice(&mut keep, "alice@example.com", dsn).expect("queue");

    let core = f.core.clone();
    tokio::task::spawn_blocking(move || mailrs_intake::notice::drain_one(&*core, NET))
        .await
        .expect("drain");
    let subject = "Undelivered Mail Returned to Sender";
    assert!(f.row("alice@example.com", subject).await.is_some());
}
