//! What a delivered message leaves in this store: the facts the shared
//! steps read from it, written where this store's views look.

use super::Fixture;

const ALICE: &str = "alice@example.com";

const REPORT: &[u8] = b"From: Carol <carol@outside.example>\r\n\
To: alice@example.com\r\n\
Subject: Quarterly report\r\n\
Message-ID: <q1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Please review the attached figures before Friday.\r\n";

async fn fixture() -> Fixture {
    let f = Fixture::new().await;
    f.account(ALICE).await;
    f
}

/// Preview, category, unread, and the time the message says it was sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_received_message_has_its_preview_category_and_date() {
    let f = fixture().await;
    f.spool("new", "1790000000.M1P1Q1.host", &[ALICE], REPORT);
    assert_eq!(f.tick().await, 1);

    let row = f.indexed(ALICE, "Quarterly report").await;
    assert_eq!(row.mailbox, "INBOX");
    assert!(!row.seen, "a received message arrived read");
    assert!(
        row.snippet
            .starts_with("Please review the attached figures"),
        "preview was {:?}",
        row.snippet
    );
    assert_eq!(row.category.as_deref(), Some("inbox"));
    assert_eq!(row.date, 1_754_352_000, "not sorted by its Date header");
    assert_eq!(row.thread_id, "q1@outside.example");
}

/// A copy of mail the user wrote is in Sent, read, with its file there.
#[tokio::test(flavor = "multi_thread")]
async fn the_users_own_mail_lands_in_sent_read() {
    let f = fixture().await;
    let own = b"From: Alice <alice@example.com>\r\n\
To: carol@outside.example\r\n\
Subject: my own note\r\n\
Message-ID: <own1@example.com>\r\n\
\r\n\
sent from my phone\r\n";
    f.spool("new", "1790000001.M1P1Q1.host", &[ALICE], own);
    assert_eq!(f.tick().await, 1);

    let row = f.indexed(ALICE, "my own note").await;
    assert_eq!(row.mailbox, "Sent");
    assert!(row.seen, "the user's own mail arrived unread");
    let file = f
        .maildir
        .path()
        .join("example.com/alice/.Sent/new/1790000001.M1P1Q1.host");
    assert!(
        file.exists(),
        "the Sent copy's file is not in the Sent folder"
    );
}

/// A held verdict is stored, holds the conversation, and reads it; a
/// verdict that does not hold leaves the conversation alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_fraud_verdict_that_holds_takes_the_conversation_out_read() {
    let f = fixture().await;
    let verdict = |held: bool| {
        serde_json::json!({
            "rules_version": "2026-08-29.1",
            "score": 9.5,
            "quarantined": held,
            "layers": [],
        })
        .to_string()
    };
    let mut env = Fixture::envelope(&[ALICE]);
    env.fraud_verdict = Some(verdict(true));
    f.spool_env("new", "1790000002.M1P1Q1.host", &env, REPORT);
    let other = b"From: Dan <dan@outside.example>\r\n\
Subject: harmless\r\n\
Message-ID: <h1@outside.example>\r\n\
\r\n\
hello\r\n";
    env.fraud_verdict = Some(verdict(false));
    f.spool_env("new", "1790000003.M1P1Q1.host", &env, other);
    assert_eq!(f.tick().await, 2);

    let held: Vec<(String,)> =
        sqlx::query_as("SELECT thread_id FROM quarantine_holds WHERE account_address = $1")
            .bind(ALICE)
            .fetch_all(&f.pool)
            .await
            .expect("holds");
    assert_eq!(held, [("q1@outside.example".to_string(),)]);
    assert!(f.indexed(ALICE, "Quarterly report").await.seen);
    assert!(!f.indexed(ALICE, "harmless").await.seen);
    let stored = f
        .core
        .mailbox
        .fraud_verdict("h1@outside.example")
        .await
        .expect("read");
    assert!(
        stored.is_some(),
        "a verdict that does not hold was not recorded"
    );
}

/// A reply joins its conversation through References even when its
/// In-Reply-To names a message this mailbox never saw; one that changes
/// the subject starts a new conversation.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_threads_by_references_and_a_new_topic_does_not() {
    let f = fixture().await;
    f.spool("new", "1790000000.M1P1Q1.host", &[ALICE], REPORT);
    f.tick().await;
    let reply = b"From: Carol <carol@outside.example>\r\n\
Subject: Re: Quarterly report\r\n\
Message-ID: <q3@outside.example>\r\n\
In-Reply-To: <q2@outside.example>\r\n\
References: <q1@outside.example> <q2@outside.example>\r\n\
\r\n\
following up\r\n";
    let new_topic = b"From: Carol <carol@outside.example>\r\n\
Subject: Annual closing\r\n\
Message-ID: <q4@outside.example>\r\n\
In-Reply-To: <q1@outside.example>\r\n\
\r\n\
another matter\r\n";
    // one at a time, so each finds the one before it
    f.spool("new", "1790000010.M1P1Q1.host", &[ALICE], reply);
    f.tick().await;
    f.spool("new", "1790000020.M1P1Q1.host", &[ALICE], new_topic);
    f.tick().await;
    let root = f.indexed(ALICE, "Quarterly report").await.thread_id;
    assert_eq!(
        f.indexed(ALICE, "Re: Quarterly report").await.thread_id,
        root
    );
    assert_ne!(f.indexed(ALICE, "Annual closing").await.thread_id, root);
}

/// Mail with no Message-ID goes by an id made from its file name, the one
/// every other reader of the file arrives at.
#[tokio::test(flavor = "multi_thread")]
async fn mail_without_a_message_id_goes_by_its_file_name() {
    let f = fixture().await;
    let alert = b"From: monitor@outside.example\r\n\
Subject: disk full\r\n\
\r\n\
92%\r\n";
    f.spool("new", "1790000005.M7P1Q1.host", &[ALICE], alert);
    assert_eq!(f.tick().await, 1);
    assert_eq!(
        f.indexed(ALICE, "disk full").await.message_id,
        "1790000005.1790000005.M7P1Q1.host@mailrs.local"
    );
}
