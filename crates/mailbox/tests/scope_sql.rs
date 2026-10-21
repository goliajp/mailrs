//! The SQL store's translation of a view's scope selects exactly the threads
//! `Scope::admits` says belong to it, the rule the kevy store keys its own
//! lists on. Every category spelling the rule knows and both sent-only
//! states are seeded, and every scope is asked.

mod common;

use std::collections::BTreeSet;

use common::{seed_domain_account, seed_mailbox, setup_pg};
use mailrs_mailbox::pg::{BackendPool, PgMailboxStore, ThreadQuery};
use mailrs_mailbox::scope::{Bucket, Scope, ThreadFacts};

const USER: &str = "reader@scope.test";

async fn message(pool: &BackendPool, mailbox: i64, uid: i32, thread: &str, category: Option<&str>) {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO messages (mailbox_id, uid, maildir_id, internal_date, date_epoch,
                               flags, size, subject, sender, recipients,
                               thread_id, message_id, in_reply_to)
         VALUES ($1, $2, $3, $4, $4, 0, 100, 'subject', 'someone@elsewhere.test', $5,
                 $6, $7, '')
         RETURNING id",
    )
    .bind(mailbox)
    .bind(uid)
    .bind(format!("mdir-{uid}"))
    .bind(1_700_000_000i64 + i64::from(uid))
    .bind(USER)
    .bind(thread)
    .bind(format!("<{uid}@elsewhere.test>"))
    .fetch_one(pool)
    .await
    .expect("message");
    if let Some(c) = category {
        sqlx::query("INSERT INTO email_analysis (message_id, category) VALUES ($1, $2)")
            .bind(id)
            .bind(c)
            .execute(pool)
            .await
            .expect("analysis");
    }
}

#[tokio::test]
async fn the_sql_scope_selects_what_the_shared_rule_admits() {
    let (_handle, pool) = setup_pg().await;
    seed_domain_account(&pool, USER).await;
    let inbox = seed_mailbox(&pool, USER, "INBOX").await;
    let sent = seed_mailbox(&pool, USER, "Sent").await;
    let junk = seed_mailbox(&pool, USER, "Junk").await;

    let categories = [
        Some("inbox"),
        Some("notification"),
        Some("Notifications"),
        Some("promotion"),
        Some("PROMOTIONS"),
        Some("spam"),
        Some("scam"),
        Some("general"),
        None,
    ];
    let mut uid = 1;
    for (i, category) in categories.into_iter().enumerate() {
        message(&pool, inbox, uid, &format!("received-{i}"), category).await;
        uid += 1;
        message(&pool, sent, uid, &format!("sent-only-{i}"), category).await;
        uid += 1;
    }
    // Filed into Junk with nothing classified: Junk by where it was filed.
    message(&pool, junk, uid, "filed-in-junk", None).await;

    let store = PgMailboxStore::new(pool.clone());
    let all = store
        .thread_rows(&ThreadQuery::all(USER))
        .await
        .expect("all");
    assert_eq!(all.len(), 2 * categories.len() + 1);

    let scopes = [
        Scope::All,
        Scope::NotJunk,
        Scope::Bucket(Bucket::Inbox),
        Scope::Bucket(Bucket::Notifications),
        Scope::Bucket(Bucket::Promotions),
        Scope::Bucket(Bucket::Junk),
        Scope::Np,
        Scope::Category("notification".into()),
        Scope::Category("Spam".into()),
        Scope::Contradiction,
    ];
    for scope in scopes {
        let want: BTreeSet<String> = all
            .iter()
            .filter(|r| {
                scope.admits(ThreadFacts {
                    category: &r.category,
                    sent_only: r.sent_count == r.message_count,
                })
            })
            .map(|r| r.thread_id.clone())
            .collect();
        let got: BTreeSet<String> = store
            .thread_rows(&ThreadQuery {
                scope: scope.clone(),
                ..ThreadQuery::all(USER)
            })
            .await
            .expect("scoped")
            .into_iter()
            .map(|r| r.thread_id)
            .collect();
        assert_eq!(got, want, "{scope:?}");
    }

    let junk_row = all
        .iter()
        .find(|r| r.thread_id == "filed-in-junk")
        .expect("junk row");
    assert_eq!(junk_row.category, "spam");
}
