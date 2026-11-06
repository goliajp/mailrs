//! A conversation of only the reader's own mail can be marked unread, on
//! both cores, as in Gmail: its row counts an unread message, it is in the
//! Unread list, and its newest message is not seen until it is read again.
//!
//! fastcore cleared `\Seen` on the newest message and set the row's unread
//! column, but counted a message the reader sent as never unread, so the
//! row sat in the Unread list showing nothing unread. The SQL core counts
//! every unseen message.
//!
//! The Unread tab (`folder=NonJunk&unread=true`) is not the Inbox: a
//! conversation the reader marked unread is in it wherever it lives
//! outside Junk, the Trash and Review, so one of only their own mail is
//! too. The Inbox and its badge still leave it out.

use mailrs_core_api::client::Client;
use mailrs_core_api::method::conversation::{ListConversationsRequest, SearchConversationsRequest};
use mailrs_core_api::method::message::FLAG_SEEN;
use mailrs_core_api::method::thread::MarkListReadRequest;
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

/// The filter all three clients' Unread tab sends.
fn unread_tab() -> ConversationFilter {
    ConversationFilter {
        folder: Some("NonJunk".into()),
        unread: Some(true),
        limit: 50,
        ..Default::default()
    }
}

fn unread_tab_list() -> ListConversationsRequest {
    ListConversationsRequest {
        filter: unread_tab(),
    }
}

/// What `core` shows Alice through the Unread tab: whether its list holds
/// the conversation, whether a search typed into it finds it, and the
/// badge's total and Inbox part.
async fn through_the_unread_tab(core: &Client) -> (bool, bool, (i64, i64)) {
    let listed = core
        .list_conversations(ALICE, &unread_tab_list())
        .await
        .expect("unread tab")
        .items
        .iter()
        .any(|r| r.thread_id == SENT);
    let search = SearchConversationsRequest {
        query: "lunch".into(),
        filter: unread_tab(),
        quarantined: false,
    };
    let found = core
        .search_conversations(ALICE, &search)
        .await
        .expect("search in the unread tab")
        .items
        .iter()
        .any(|r| r.thread_id == SENT);
    let badge = core.unseen_count(ALICE).await.expect("badge");
    (listed, found, (badge.count, badge.inbox))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_conversation_of_only_your_own_mail_marked_unread_is_in_the_unread_tab() {
    let cores = harness::start_on(
        "mem://own-unread-tab-fastcore-side-state",
        "mem://own-unread-tab-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    mail::seed(&cores, &mut d).await;

    let cores_by_name = [("fastcore", &cores.fastcore), ("pg", &cores.pg_core)];
    let mut wrong = Vec::new();
    let mut badge_before = Vec::new();
    for (name, core) in cores_by_name {
        let (listed, found, badge) = through_the_unread_tab(core).await;
        if listed || found {
            wrong.push(format!(
                "{name}: delivered read, the Unread tab lists it {listed}, finds it {found}"
            ));
        }
        badge_before.push(badge);
    }

    d.both(
        "mark_thread_unread own",
        cores.fastcore.mark_thread_unread(ALICE, SENT).await,
        cores.pg_core.mark_thread_unread(ALICE, SENT).await,
        MINTED,
    );
    for ((name, core), before) in cores_by_name.into_iter().zip(&badge_before) {
        let (listed, found, badge) = through_the_unread_tab(core).await;
        if !listed || !found {
            wrong.push(format!(
                "{name}: marked unread, the Unread tab lists it {listed}, finds it {found}"
            ));
        }
        if badge != *before {
            wrong.push(format!(
                "{name}: marking it unread moved the badge from {before:?} to {badge:?}"
            ));
        }
    }
    d.both(
        "list_conversations unread tab after mark unread",
        cores
            .fastcore
            .list_conversations(ALICE, &unread_tab_list())
            .await,
        cores
            .pg_core
            .list_conversations(ALICE, &unread_tab_list())
            .await,
        MINTED,
    );

    let req = MarkListReadRequest {
        filter: unread_tab(),
        quarantined: false,
    };
    for (name, core) in cores_by_name {
        core.mark_list_conversations_read(ALICE, &req)
            .await
            .expect("mark the unread tab read");
        let got = state(core).await;
        if got != (0, false, true) {
            wrong.push(format!(
                "{name}: Unread tab marked read, it reads (unread count, in Unread, newest seen) {got:?}"
            ));
        }
        let (listed, _, _) = through_the_unread_tab(core).await;
        if listed {
            wrong.push(format!("{name}: Unread tab marked read, it still lists it"));
        }
    }
    compare(&cores, &mut d, "after marking the unread tab read").await;

    wrong.extend(d.0);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
