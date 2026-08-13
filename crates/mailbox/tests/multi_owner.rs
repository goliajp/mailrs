//! Per-user conversation state on a thread two accounts both received.
//!
//! This exists to settle a sizing question with a measurement instead of a
//! reading. `.claude/rfcs/20260730-per-user-thread-state.md` fixed a
//! multi-owner defect on the kevy side — putting a conversation away did it
//! for everyone who could see it — and recorded that "the PG lane still has
//! the multi-owner bug those fixed". The plan for reactivating this lane
//! therefore budgeted two new tables, `threads` and `thread_users`.
//!
//! Reading the schema says otherwise, and three layers of it agree:
//!
//! - `messages` is one row **per mailbox** (`mailbox_id NOT NULL REFERENCES
//!   mailboxes`, `UNIQUE(mailbox_id, uid)`), and a mailbox belongs to exactly
//!   one account. So a thread two accounts received has two disjoint sets of
//!   rows.
//! - `list_conversations` reads `FROM messages m JOIN mailboxes mb ON
//!   m.mailbox_id = mb.id` with `mb.user_address = $N`, then
//!   `GROUP BY m.thread_id HAVING BOOL_OR(m.archived) = false`. The aggregate
//!   therefore only ever sees one owner's rows.
//! - the per-user fields land on those rows: `archived` and `pinned` are
//!   columns, starred is the IMAP `\Flagged` bit (`m.flags & 4`), unread is
//!   `flags & 1`, and snooze already has `(thread_id, account_address)` as its
//!   primary key.
//!
//! If that holds, PG gets per-user state from normalisation — the kevy
//! membership row is a denormalisation patch for a shared-hash model this side
//! does not have — and the schema work for reactivation is two columns rather
//! than two tables. That is a large enough difference to be worth a test
//! rather than a conclusion, and the test is worth keeping either way: it pins
//! the property the July RFC was about, on the side nobody had checked.

mod common;

use common::{seed_domain_account, seed_mailbox, setup_pg};
use mailrs_mailbox::pg::PgMailboxStore;

const ALICE: &str = "alice@multi.test";
const BOB: &str = "bob@multi.test";
const TID: &str = "shared-thread";

/// One message, delivered into one owner's mailbox. Flags are IMAP bits:
/// 1 = `\Seen`, 4 = `\Flagged`.
async fn deliver(
    pool: &mailrs_mailbox::pg::BackendPool,
    mailbox_id: i64,
    uid: i32,
    flags: i32,
    archived: bool,
) {
    sqlx::query(
        "INSERT INTO messages (
            mailbox_id, uid, maildir_id, internal_date, date_epoch,
            flags, size, subject, sender, recipients,
            thread_id, message_id, in_reply_to, archived
         ) VALUES ($1, $2, $3, 1700000000, 1700000000,
                   $4, 100, 'shared subject', 'sender@elsewhere.test',
                   'alice@multi.test, bob@multi.test',
                   $5, $6, '', $7)",
    )
    .bind(mailbox_id)
    .bind(uid)
    .bind(format!("mdir-{mailbox_id}-{uid}"))
    .bind(flags)
    .bind(TID)
    .bind(format!("<shared-{uid}@elsewhere.test>"))
    .bind(archived)
    .execute(pool)
    .await
    .expect("deliver");
}

async fn thread_ids(store: &PgMailboxStore, user: &str, archived: bool) -> Vec<String> {
    store
        .list_conversations(user, 50, None, None, None, archived, None, None, None, None)
        .await
        .expect("list_conversations")
        .into_iter()
        .map(|c| c.thread_id)
        .collect()
}

/// Archiving is one reader's decision.
#[tokio::test]
async fn one_owner_archiving_does_not_archive_it_for_the_other() {
    let (_h, pool) = setup_pg().await;
    for u in [ALICE, BOB] {
        seed_domain_account(&pool, u).await;
    }
    let a_box = seed_mailbox(&pool, ALICE, "INBOX").await;
    let b_box = seed_mailbox(&pool, BOB, "INBOX").await;

    // The same conversation, in both inboxes. Alice has put hers away.
    deliver(&pool, a_box, 1, 0, true).await;
    deliver(&pool, b_box, 1, 0, false).await;

    let store = PgMailboxStore::new(pool.clone());

    assert_eq!(
        thread_ids(&store, ALICE, false).await,
        Vec::<String>::new(),
        "alice archived it, so her inbox must not list it"
    );
    assert_eq!(
        thread_ids(&store, ALICE, true).await,
        vec![TID.to_string()],
        "and her archive must"
    );
    assert_eq!(
        thread_ids(&store, BOB, false).await,
        vec![TID.to_string()],
        "bob did not archive it — if this is empty, one reader's archive \
         removed the conversation from another's inbox, which is the \
         multi-owner defect the kevy side was fixed for"
    );
    assert_eq!(
        thread_ids(&store, BOB, true).await,
        Vec::<String>::new(),
        "and it must not be in bob's archive"
    );
}

/// Starred is one reader's decision too, and it is the flag bit rather than a
/// column — so this also pins where "starred" lives on this side.
#[tokio::test]
async fn starring_is_per_reader() {
    let (_h, pool) = setup_pg().await;
    for u in [ALICE, BOB] {
        seed_domain_account(&pool, u).await;
    }
    let a_box = seed_mailbox(&pool, ALICE, "INBOX").await;
    let b_box = seed_mailbox(&pool, BOB, "INBOX").await;

    // 4 = IMAP \Flagged. Alice starred hers.
    deliver(&pool, a_box, 1, 4, false).await;
    deliver(&pool, b_box, 1, 0, false).await;

    let store = PgMailboxStore::new(pool.clone());
    async fn starred(store: &PgMailboxStore, user: &str) -> Vec<String> {
        store
            .list_conversations(
                user,
                50,
                None,
                None,
                None,
                false,
                None,
                None,
                Some(true),
                None,
            )
            .await
            .expect("list starred")
            .into_iter()
            .map(|c| c.thread_id)
            .collect()
    }

    assert_eq!(
        starred(&store, ALICE).await,
        vec![TID.to_string()],
        "alice starred it"
    );
    assert_eq!(
        starred(&store, BOB).await,
        Vec::<String>::new(),
        "bob did not — a star that crosses readers is the same defect wearing \
         a different flag"
    );
}

/// Unread counts are per reader, and they are counted from the reader's own
/// rows rather than from the thread.
#[tokio::test]
async fn unread_counts_are_per_reader() {
    let (_h, pool) = setup_pg().await;
    for u in [ALICE, BOB] {
        seed_domain_account(&pool, u).await;
    }
    let a_box = seed_mailbox(&pool, ALICE, "INBOX").await;
    let b_box = seed_mailbox(&pool, BOB, "INBOX").await;

    // Two messages in the thread. Alice has read both; bob neither.
    deliver(&pool, a_box, 1, 1, false).await;
    deliver(&pool, a_box, 2, 1, false).await;
    deliver(&pool, b_box, 1, 0, false).await;
    deliver(&pool, b_box, 2, 0, false).await;

    let store = PgMailboxStore::new(pool.clone());

    let a = store
        .list_conversations(ALICE, 50, None, None, None, false, None, None, None, None)
        .await
        .expect("list alice");
    let b = store
        .list_conversations(BOB, 50, None, None, None, false, None, None, None, None)
        .await
        .expect("list bob");

    assert_eq!(a.len(), 1, "one thread for alice");
    assert_eq!(b.len(), 1, "one thread for bob");
    assert_eq!(a[0].unread_count, 0, "alice read both");
    assert_eq!(b[0].unread_count, 2, "bob read neither");
    assert_eq!(
        store.count_unseen(ALICE).await.unwrap(),
        0,
        "and the badge agrees for alice"
    );
    assert_eq!(
        store.count_unseen(BOB).await.unwrap(),
        1,
        "the badge counts threads, not messages, so bob has one"
    );
}
