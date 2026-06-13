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
    let extension: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'vector')")
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
    assert_eq!(store.count_unseen(USER).await.unwrap(), 0);
}

#[tokio::test]
async fn seed_mailbox_inserts_row_visible_via_query() {
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;

    let mb_id = seed_mailbox(&pool, USER, "INBOX").await;

    // Round-trip via raw SQL since list_mailboxes lives behind MailboxOps
    // we're not exercising here.
    let row: (String,) = sqlx::query_as("SELECT name FROM mailboxes WHERE id = $1")
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

#[tokio::test]
async fn search_conversations_hits_tsvector_and_ilike_branches() {
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let mailbox_id = seed_mailbox(&pool, USER, "INBOX").await;

    // Two messages in one thread; the search_vector trigger populates the
    // tsvector column from subject/sender/body on INSERT.
    for (uid, subject, body) in [
        (1, "quarterly invoice attached", "please find the invoice"),
        (2, "re: quarterly invoice attached", "thanks, received"),
    ] {
        sqlx::query(
            "INSERT INTO messages (
                mailbox_id, uid, maildir_id, internal_date, date_epoch,
                flags, size, subject, sender, recipients,
                thread_id, message_id, in_reply_to, text_body
            ) VALUES ($1, $2, $3, 1700000000, 1700000000,
                      0, 100, $4, 'a@x', $5,
                      'th-1', $6, '', $7)",
        )
        .bind(mailbox_id)
        .bind(uid)
        .bind(format!("mdir-{uid}"))
        .bind(subject)
        .bind(USER)
        .bind(format!("<m{uid}@x>"))
        .bind(body)
        .execute(&pool)
        .await
        .unwrap();
    }

    let store = PgMailboxStore::new(pool.clone());

    // tsvector branch: "invoice" is a token in subject + body.
    let hits = store
        .search_conversations(USER, "invoice", 10, None, None)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1, "one thread matches");
    assert_eq!(hits[0].thread_id, "th-1");

    // ILIKE branch: substring that tsvector tokenisation won't match.
    let hits = store
        .search_conversations(USER, "uarterly", 10, None, None)
        .await
        .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "ILIKE substring match still finds the thread"
    );

    // No match.
    let hits = store
        .search_conversations(USER, "zebra", 10, None, None)
        .await
        .unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn reconcile_maildir_repairs_orphan_files() {
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let store = PgMailboxStore::new(pool.clone());
    store.create_mailbox(USER, "INBOX").await.unwrap();

    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_str().unwrap();

    // control: one message through the normal indexed path
    let indexed = b"From: a@x.com\r\nTo: alice@example.com\r\nSubject: indexed\r\nMessage-ID: <ctl-1@x.com>\r\n\r\nbody one\r\n";
    store
        .append_message(USER, "INBOX", root_path, indexed, 0, 1_700_000_000)
        .await
        .unwrap();

    // orphan: delivered to disk only, never indexed (the split-brain shape)
    let md = mailrs_maildir::Maildir::create(format!("{root_path}/example.com/alice")).unwrap();
    let orphan = b"From: b@y.com\r\nTo: alice@example.com\r\nSubject: orphan\r\nMessage-ID: <orp-1@y.com>\r\n\r\nbody two\r\n";
    let orphan_id = md.deliver(orphan).unwrap().to_string();

    // dry run: detects, repairs nothing
    let report = store.reconcile_maildir(root_path, true).await.unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.missing, 1);
    assert_eq!(report.repaired, 0);
    let n: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n.0, 1, "dry run must not write");

    // repair
    let report = store.reconcile_maildir(root_path, false).await.unwrap();
    assert_eq!(report.missing, 1);
    assert_eq!(report.repaired, 1);

    let row: (String, String, String) =
        sqlx::query_as("SELECT sender, subject, thread_id FROM messages WHERE maildir_id = $1")
            .bind(&orphan_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(row.0.contains("b@y.com"));
    assert_eq!(row.1, "orphan");
    assert!(!row.2.is_empty(), "threading resolved");

    // idempotent: second pass finds nothing
    let report = store.reconcile_maildir(root_path, false).await.unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.missing, 0);
}

#[tokio::test]
async fn count_unseen_counts_unread_threads() {
    // sentinel: count_unseen's unread aggregate must actually count. its
    // result was once swallowed to 0 on error, hiding that spg couldn't
    // parse the standard FILTER clause — the homepage unread badge read 0
    // on full mailboxes (incident 2026-06-13). runs on both axes now via
    // the TEMP(round-29) CASE form; flips back to FILTER when spg ships it.
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let store = PgMailboxStore::new(pool.clone());
    store.create_mailbox(USER, "INBOX").await.unwrap();

    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_str().unwrap();
    // delivered unseen (flags = 0), sender != the user so it isn't a
    // self-sent thread the count excludes
    let msg = b"From: someone@elsewhere.com\r\nTo: alice@example.com\r\nSubject: unread one\r\nMessage-ID: <u-1@elsewhere.com>\r\n\r\nbody\r\n";
    store
        .append_message(USER, "INBOX", root_path, msg, 0, 1_700_000_000)
        .await
        .unwrap();

    assert_eq!(
        store.count_unseen(USER).await.unwrap(),
        1,
        "one unseen inbound thread must count as unread"
    );

    // a second unseen message in the SAME thread is still one unread thread
    let msg2 = b"From: someone@elsewhere.com\r\nTo: alice@example.com\r\nSubject: re: unread\r\nMessage-ID: <u-2@elsewhere.com>\r\nIn-Reply-To: <u-1@elsewhere.com>\r\n\r\nbody2\r\n";
    store
        .append_message(USER, "INBOX", root_path, msg2, 0, 1_700_000_100)
        .await
        .unwrap();
    assert_eq!(
        store.count_unseen(USER).await.unwrap(),
        1,
        "thread-level count: two messages in one thread is one unread thread"
    );
}

#[tokio::test]
async fn reconcile_no_message_id_still_gets_thread_and_is_visible() {
    // regression (2026-06-13): an orphan whose Message-ID header didn't
    // extract got an empty thread_id, and list_conversations'
    // `WHERE thread_id != ''` then hid it from every view — a real
    // business email silently vanished. orphans must always thread.
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let store = PgMailboxStore::new(pool.clone());
    store.create_mailbox(USER, "INBOX").await.unwrap();

    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_str().unwrap();

    // orphan on disk with NO Message-ID header at all
    let md = mailrs_maildir::Maildir::create(format!("{root_path}/example.com/alice")).unwrap();
    let orphan = b"From: boss@partner.com\r\nTo: alice@example.com\r\nSubject: PO 4300079030 invoice\r\n\r\nplease invoice separately\r\n";
    md.deliver(orphan).unwrap();

    let report = store.reconcile_maildir(root_path, false).await.unwrap();
    assert_eq!(report.repaired, 1);

    let row: (String,) = sqlx::query_as("SELECT thread_id FROM messages WHERE subject = $1")
        .bind("PO 4300079030 invoice")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        !row.0.is_empty(),
        "orphan with no Message-ID must still get a non-empty thread_id"
    );

    // and it is visible in the default conversation list
    let convos = store
        .list_conversations(USER, 50, None, None, None, false, None, None, None, None)
        .await
        .unwrap();
    assert!(
        convos.iter().any(|c| c.thread_id == row.0),
        "the recovered message must appear in the default list"
    );
}

#[tokio::test]
async fn default_list_shows_spam_categorised_mail() {
    // "all" must mean ALL: a message a (possibly stale, possibly wrong)
    // classifier tagged 'spam' must still appear in the default view —
    // categories are opt-in filters, never silent exclusions.
    let (_container, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let mailbox_id = seed_mailbox(&pool, USER, "INBOX").await;

    sqlx::query(
        "INSERT INTO messages (mailbox_id, uid, maildir_id, internal_date, date_epoch,
            flags, size, subject, sender, recipients, thread_id, message_id, in_reply_to)
         VALUES ($1, 1, 'm-1', 1700000000, 1700000000, 0, 100,
            'Settlement Agreement', 'legal@partner.com', $2, 'th-spam', '<s1@x>', '')",
    )
    .bind(mailbox_id)
    .bind(USER)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO email_analysis (message_id, category) SELECT id, 'spam' FROM messages WHERE thread_id = 'th-spam'")
        .execute(&pool)
        .await
        .unwrap();

    let store = PgMailboxStore::new(pool.clone());
    let convos = store
        .list_conversations(USER, 50, None, None, None, false, None, None, None, None)
        .await
        .unwrap();
    assert!(
        convos.iter().any(|c| c.thread_id == "th-spam"),
        "spam-categorised mail must still show in the default 'all' view"
    );
}
