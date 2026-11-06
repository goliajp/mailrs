//! A conversation of only the reader's own mail can be marked unread, on
//! both cores, as in Gmail: its row counts an unread message, it is in the
//! Unread list, and its newest message is not seen until it is read again.
//!
//! fastcore cleared `\Seen` on the newest message and set the row's unread
//! column, but counted a message the reader sent as never unread, so the
//! row sat in the Unread list showing nothing unread. The SQL core counts
//! every unseen message.

use mailrs_core_api::client::Client;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::message::FLAG_SEEN;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::{ALICE, accounts, mail};

/// Ids each store mints, and the action counts, which the two count
/// differently (messages touched against rows).
const MINTED: &[&str] = &[
    "id",
    "mailbox_id",
    "created_at",
    "modseq",
    "new_modseq",
    "affected",
];

/// Seeded by `mail::seed`: one message, from Alice, delivered read.
const SENT: &str = "t-sent@example.test";

fn all_mail(unread: Option<bool>) -> ListConversationsRequest {
    ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some("all".into()),
            unread,
            limit: 50,
            ..Default::default()
        },
    }
}

/// What `core` shows Alice of the conversation: its unread count in All
/// Mail, whether All Mail's unread view lists it, and whether its newest
/// message is seen.
async fn state(core: &Client) -> (u32, bool, bool) {
    let rows = core
        .list_conversations(ALICE, &all_mail(None))
        .await
        .expect("list")
        .items;
    let row = rows
        .iter()
        .find(|r| r.thread_id == SENT)
        .unwrap_or_else(|| panic!("All Mail holds {SENT}: {rows:?}"));
    let in_unread = core
        .list_conversations(ALICE, &all_mail(Some(true)))
        .await
        .expect("unread list")
        .items
        .iter()
        .any(|r| r.thread_id == SENT);
    let messages = core
        .list_thread_messages(ALICE, SENT)
        .await
        .expect("messages")
        .items;
    let newest = messages
        .iter()
        .max_by_key(|m| m.internal_date)
        .expect("a message");
    (row.unread_count, in_unread, newest.flags & FLAG_SEEN != 0)
}

/// Both cores' views of All Mail and the badge, compared.
async fn compare(c: &Cores, d: &mut Diffs, step: &str) {
    for unread in [None, Some(true)] {
        d.both(
            &format!("list_conversations all mail unread={unread:?} {step}"),
            c.fastcore
                .list_conversations(ALICE, &all_mail(unread))
                .await,
            c.pg_core.list_conversations(ALICE, &all_mail(unread)).await,
            MINTED,
        );
    }
    d.both(
        &format!("unseen_count {step}"),
        c.fastcore.unseen_count(ALICE).await,
        c.pg_core.unseen_count(ALICE).await,
        &[],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_conversation_of_only_your_own_mail_marks_unread_on_both_cores() {
    let cores = harness::start_on(
        "mem://own-unread-fastcore-side-state",
        "mem://own-unread-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    mail::seed(&cores, &mut d).await;

    let cores_by_name = [("fastcore", &cores.fastcore), ("pg", &cores.pg_core)];
    let mut wrong = Vec::new();
    for (name, core) in cores_by_name {
        let got = state(core).await;
        if got != (0, false, true) {
            wrong.push(format!("{name}: delivered, it reads {got:?}"));
        }
    }

    d.both(
        "mark_thread_unread own",
        cores.fastcore.mark_thread_unread(ALICE, SENT).await,
        cores.pg_core.mark_thread_unread(ALICE, SENT).await,
        MINTED,
    );
    for (name, core) in cores_by_name {
        let got = state(core).await;
        if got != (1, true, false) {
            wrong.push(format!(
                "{name}: marked unread, it reads (unread count, in Unread, newest seen) {got:?}"
            ));
        }
    }
    compare(&cores, &mut d, "after mark unread").await;

    d.both(
        "mark_thread_read own",
        cores.fastcore.mark_thread_read(ALICE, SENT).await,
        cores.pg_core.mark_thread_read(ALICE, SENT).await,
        MINTED,
    );
    for (name, core) in cores_by_name {
        let got = state(core).await;
        if got != (0, false, true) {
            wrong.push(format!("{name}: read again, it reads {got:?}"));
        }
    }
    compare(&cores, &mut d, "after mark read").await;

    wrong.extend(d.0);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
