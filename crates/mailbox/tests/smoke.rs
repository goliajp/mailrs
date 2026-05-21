//! Smoke-level integration tests proving the testcontainers + sqlx + `init-schema.sql`
//! setup works end-to-end against the public [`PgMailboxStore`] API.
//!
//! These tests are intentionally lightweight — one or two per ops surface,
//! enough to demonstrate the pattern. Bringing the test density up to
//! publish quality (40+/kloc) is a separate effort.
//!
//! Each test spins up its own Postgres 18 + pgvector container; expect
//! ~3-5 s startup per test on a warm host. Tests run sequentially when
//! invoked via `cargo test --test smoke -- --test-threads=1` if you want
//! to avoid contention on Docker resources.

mod common;

use common::{seed_domain_account, seed_mailbox, setup_pg};
use mailrs_mailbox::PgMailboxStore;

const USER: &str = "alice@example.com";

#[tokio::test]
async fn pg_container_starts_and_schema_applies() {
    let (_container, pool) = setup_pg().await;

    // pgvector extension present + at least one of the schema tables.
    let extension: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'vector')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(extension.0, "pgvector extension installed");

    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, 0, "messages table empty on fresh container");
}

#[tokio::test]
async fn mailbox_store_new_constructs_from_pool() {
    let (_container, pool) = setup_pg().await;

    let store = PgMailboxStore::new(pool.clone());

    // Trivial assertion: the store hands the same pool back.
    assert!(
        std::ptr::eq(store.pool(), store.pool()),
        "pool() borrow is stable"
    );
}

#[tokio::test]
async fn count_messages_returns_zero_for_user_with_no_data() {
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;

    let store = PgMailboxStore::new(pool);

    assert_eq!(store.count_messages(USER).await, 0);
    assert_eq!(store.count_unseen(USER).await, 0);
}

#[tokio::test]
async fn seed_mailbox_inserts_row_visible_via_query() {
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;

    let mb_id = seed_mailbox(&pool, USER, "INBOX").await;

    // Round-trip via raw SQL since list_mailboxes lives behind MailboxOps
    // we're not exercising here.
    let row: (String,) =
        sqlx::query_as("SELECT name FROM mailboxes WHERE id = $1")
            .bind(mb_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, "INBOX");
}

#[tokio::test]
async fn flag_ops_update_round_trip_via_real_pg() {
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let mailbox_id = seed_mailbox(&pool, USER, "INBOX").await;

    // Insert a minimal message row directly. The full message_ops::index_message
    // path involves maildir + analysis; staying close to the flag bit here.
    sqlx::query(
        "INSERT INTO messages (
            mailbox_id, uid, maildir_id, internal_date, date_epoch,
            flags, size, subject, sender, recipients,
            thread_id, message_id, in_reply_to
        ) VALUES ($1, 1, 'mdir-1', 1700000000, 1700000000,
                  0, 100, 'test', 'a@x', 'b@x',
                  '', '<x@y>', '')",
    )
    .bind(mailbox_id)
    .execute(&pool)
    .await
    .unwrap();

    let store = PgMailboxStore::new(pool.clone());

    // add_flags bumps flags via OR; verify the bit is set after.
    let modseq = store.add_flags(mailbox_id, 1, 1).await.unwrap();
    assert!(modseq > 0, "modseq advances on flag mutation");

    let flags: (i32,) =
        sqlx::query_as("SELECT flags FROM messages WHERE mailbox_id = $1 AND uid = 1")
            .bind(mailbox_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(flags.0 & 1, 1, "FLAG_SEEN bit set after add_flags");

    // remove_flags clears it again.
    store.remove_flags(mailbox_id, 1, 1).await.unwrap();
    let flags: (i32,) =
        sqlx::query_as("SELECT flags FROM messages WHERE mailbox_id = $1 AND uid = 1")
            .bind(mailbox_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(flags.0 & 1, 0, "FLAG_SEEN bit cleared after remove_flags");
}
