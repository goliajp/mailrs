//! Whether the reader has written in a conversation, as the SQL core's
//! sections read it: what the push policy asks at delivery.

mod common;

use common::{seed_domain_account, seed_mailbox, setup_pg};
use mailrs_mailbox::InsertMessage;
use mailrs_mailbox::pg::{ArrivalFacts, PgMailboxStore};
use mailrs_mailbox::section::MessageFacts;

const ME: &str = "me@wrote.test";

async fn arrive(store: &PgMailboxStore, mailbox: &str, uid: u32, sender: &str) {
    let mid = format!("m{uid}@wrote.test");
    let blob = format!("blob-{uid}");
    let input = InsertMessage {
        user: ME,
        mailbox_name: mailbox,
        blob_ref: &blob,
        sender,
        recipients: ME,
        subject: "hello",
        size: 100,
        date: 1_700_000_000 + i64::from(uid),
        internal_date: 1_700_000_000 + i64::from(uid),
        message_id: &mid,
        in_reply_to: "",
        thread_id: "t",
        flags: 0,
    };
    let facts = ArrivalFacts {
        snippet: "",
        category: "inbox",
        sender_trust: "",
        attachments: None,
        section: Some(MessageFacts::default()),
        from_addr: "",
    };
    store.record_arrival(input, facts).await.expect("arrival");
}

#[tokio::test]
async fn a_message_of_mine_in_sent_means_i_wrote_in_the_conversation() {
    let (_h, pool) = setup_pg().await;
    seed_domain_account(&pool, ME).await;
    for name in ["INBOX", "Sent"] {
        seed_mailbox(&pool, ME, name).await;
    }
    let store = PgMailboxStore::new(pool);
    arrive(&store, "INBOX", 1, "ann@y.test").await;
    assert!(!store.reader_wrote_in(ME, "t").await.unwrap());
    arrive(&store, "Sent", 2, ME).await;
    assert!(store.reader_wrote_in(ME, "t").await.unwrap());
    assert!(!store.reader_wrote_in(ME, "other").await.unwrap());
}
