//! Purging a deleted group's reply claims in SQL takes every conversation
//! of that group and no other group's: a tag is `{gid}:{conv}`, so group 4
//! must not take group 40's. A dry run counts and removes nothing. Runs on
//! both backend axes.

mod common;

use common::setup_pg;
use mailrs_mailbox::pg::PgMailboxStore;
use mailrs_mailbox::shared_reply::Claim;

#[tokio::test]
async fn a_group_purge_takes_its_reply_claims_and_no_other_groups() {
    let (_h, pool) = setup_pg().await;
    let store = PgMailboxStore::new(pool.clone());
    for tag in ["4:1", "4:2", "40:1", "5:1"] {
        let won = store
            .claim_reply(tag, "a@x.test", 0, false, 1)
            .await
            .expect("claim");
        assert!(matches!(won, Claim::Won { .. }), "{tag}");
    }
    assert_eq!(
        store.purge_group_replies(4, true).await.expect("dry run"),
        2
    );
    assert_eq!(store.reply_state("4:1").await.expect("state").seq, 1);
    assert_eq!(store.purge_group_replies(4, false).await.expect("purge"), 2);
    assert_eq!(store.reply_state("4:1").await.expect("state").seq, 0);
    assert_eq!(store.reply_state("4:2").await.expect("state").seq, 0);
    assert_eq!(
        store.reply_state("40:1").await.expect("state").seq,
        1,
        "group 40 is not group 4"
    );
    assert_eq!(store.reply_state("5:1").await.expect("state").seq, 1);
    assert_eq!(store.purge_group_replies(4, false).await.expect("again"), 0);
}
