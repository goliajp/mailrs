//! A snoozed conversation is findable while it sleeps.
//!
//! Asleep means filed away, not gone. The client says so itself:
//!
//! > A thread that is asleep is filed away, so it shows up in Archived and
//! > nowhere else. Without saying when it comes back, that reads as an ordinary
//! > archived thread and the way out is not obvious.
//! > — `web/src/components/conversation-item.tsx`
//!
//! It renders a "Snoozed until <date>" badge on the row to tell the two apart,
//! which needs the row to be in the list at all.
//!
//! This lane excluded an active snooze from *every* view, archived included, so
//! a conversation somebody put away for a week was gone for that week — no
//! inbox, no archive, nowhere. Found while comparing the two cores: the kevy
//! side does not filter snoozed threads out of its list, and the reason it needs
//! a periodic wake is precisely that the row stays and the field has to be
//! cleared.
//!
//! Runs on both backend axes, because it is a property of the query and not of
//! the engine underneath it.

mod common;

use common::{seed_domain_account, seed_mailbox, setup_pg};
use mailrs_mailbox::pg::PgMailboxStore;

const USER: &str = "sleeper@snooze.test";
const AWAKE: &str = "awake-thread";
const ASLEEP: &str = "asleep-thread";

async fn deliver(pool: &mailrs_mailbox::pg::BackendPool, mailbox_id: i64, uid: i32, tid: &str) {
    sqlx::query(
        "INSERT INTO messages (
            mailbox_id, uid, maildir_id, internal_date, date_epoch,
            flags, size, subject, sender, recipients,
            thread_id, message_id, in_reply_to, archived
         ) VALUES ($1, $2, $3, 1700000000, 1700000000,
                   0, 100, 'subject', 'sender@elsewhere.test', $4,
                   $5, $6, '', false)",
    )
    .bind(mailbox_id)
    .bind(uid)
    .bind(format!("mdir-{tid}-{uid}"))
    .bind(USER)
    .bind(tid)
    .bind(format!("<{tid}-{uid}@elsewhere.test>"))
    .execute(pool)
    .await
    .expect("deliver");
}

async fn thread_ids(store: &PgMailboxStore, archived: bool) -> Vec<String> {
    store
        .list_conversations(USER, 50, None, None, None, archived, None, None, None, None)
        .await
        .expect("list_conversations")
        .into_iter()
        .map(|c| c.thread_id)
        .collect()
}

#[tokio::test]
async fn a_sleeping_thread_leaves_the_inbox_and_appears_in_archived() {
    let (_h, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let mbox = seed_mailbox(&pool, USER, "INBOX").await;
    deliver(&pool, mbox, 1, AWAKE).await;
    deliver(&pool, mbox, 2, ASLEEP).await;

    let store = PgMailboxStore::new(pool.clone());

    // Both in the inbox to begin with, neither archived.
    let mut before = thread_ids(&store, false).await;
    before.sort();
    assert_eq!(before, vec![ASLEEP.to_string(), AWAKE.to_string()]);
    assert!(thread_ids(&store, true).await.is_empty());

    store
        .snooze_thread(USER, ASLEEP, chrono::Utc::now() + chrono::Duration::days(7))
        .await
        .expect("snooze");

    assert_eq!(
        thread_ids(&store, false).await,
        vec![AWAKE.to_string()],
        "an active snooze must leave the inbox"
    );
    assert_eq!(
        thread_ids(&store, true).await,
        vec![ASLEEP.to_string()],
        "…and must be findable in Archived while it sleeps. Empty here means a \
         conversation somebody put away is in no view at all until its time \
         passes, which is what this lane did before the exclusion was scoped to \
         the non-archived query."
    );
}

#[tokio::test]
async fn the_wake_brings_back_what_is_due_and_costs_nothing_when_idle() {
    // Once a snooze archives the thread, a predicate on the read cannot bring it
    // back: the row is archived, and "archived because asleep, and no longer
    // asleep" is not a state the list can tell from "archived on purpose". So
    // this lane needs the same wake the kevy one has — an earlier version of
    // this file asserted the opposite, on the strength of the pre-fix behaviour
    // where a snooze archived nothing.
    let (_h, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let mbox = seed_mailbox(&pool, USER, "INBOX").await;
    deliver(&pool, mbox, 1, ASLEEP).await;

    let store = PgMailboxStore::new(pool.clone());

    // Nothing snoozed: the tick must do no work at all, not merely no harm.
    assert_eq!(
        store.wake_snoozed().await.expect("idle wake"),
        0,
        "an idle tick must report no work — a counter that cannot come out zero \
         is the shape that turned the maildir sweep's own idle report into the \
         noise hiding it"
    );

    store
        .snooze_thread(
            USER,
            ASLEEP,
            chrono::Utc::now() - chrono::Duration::hours(1),
        )
        .await
        .expect("snooze in the past");
    assert!(
        thread_ids(&store, false).await.is_empty(),
        "asleep means away, even for a time already past — until the wake runs"
    );

    assert_eq!(
        store.wake_snoozed().await.expect("wake"),
        1,
        "one thread was due"
    );
    assert_eq!(
        thread_ids(&store, false).await,
        vec![ASLEEP.to_string()],
        "and it is back in the inbox"
    );
    assert!(
        thread_ids(&store, true).await.is_empty(),
        "and no longer in Archived — the wake clears the flag it set"
    );

    // Again, with nothing left due.
    assert_eq!(
        store.wake_snoozed().await.expect("second wake"),
        0,
        "the wake must converge: a second pass over the same state does nothing"
    );
}

#[tokio::test]
async fn unsnoozing_by_hand_brings_it_back_too() {
    let (_h, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let mbox = seed_mailbox(&pool, USER, "INBOX").await;
    deliver(&pool, mbox, 1, ASLEEP).await;

    let store = PgMailboxStore::new(pool.clone());
    store
        .snooze_thread(USER, ASLEEP, chrono::Utc::now() + chrono::Duration::days(7))
        .await
        .expect("snooze");
    store.unsnooze_thread(USER, ASLEEP).await.expect("unsnooze");

    assert_eq!(
        thread_ids(&store, false).await,
        vec![ASLEEP.to_string()],
        "waking it by hand must clear the archive the snooze set, or the thread \
         stays filed away with nothing recording why"
    );
}
