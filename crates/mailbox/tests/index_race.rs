//! Two writers indexing the same delivered file at once — reconcile racing
//! the live path, or a migration's delivery racing reconcile — both get the
//! one row, and neither an error.

mod common;
use common::{seed_domain_account, setup_pg};
use mailrs_mailbox::PgMailboxStore;

const USER: &str = "race@example.com";

#[tokio::test]
async fn concurrent_indexes_of_one_file_share_its_row() {
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let store = std::sync::Arc::new(PgMailboxStore::new(pool.clone()));
    store.create_mailbox(USER, "INBOX").await.unwrap();

    let writers: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .index_message(
                        USER,
                        "INBOX",
                        "raced-maildir-1",
                        "a@x.com",
                        USER,
                        "subject",
                        100,
                        1_700_000_000,
                        "<raced@x.com>",
                        "",
                        "th-raced",
                    )
                    .await
            })
        })
        .collect();
    let mut uids = Vec::new();
    for w in writers {
        uids.push(w.await.unwrap().expect("every writer succeeds"));
    }
    uids.dedup();
    assert_eq!(uids.len(), 1, "all writers got the one row's uid: {uids:?}");

    let (rows,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM messages WHERE maildir_id = 'raced-maildir-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rows, 1);
}
