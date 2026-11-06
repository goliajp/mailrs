//! Starred and Pinned, on both cores, as in Gmail: a conversation the
//! reader starred or pinned is in the list wherever it lives, archived
//! or of only their own mail, but not in Junk, the Trash or Review.
//!
//! Both cores read `folder=NonJunk&starred=true` (and `pinned`) as the
//! Inbox-shaped "every bucket but Junk", so a conversation of only the
//! reader's own mail and an archived one never showed. The Inbox, the
//! JMAP Inbox (`folder=NonJunk` alone) and the badge are not lists of a
//! flag and keep both out.
//!
//! An account filter narrows a list to some mailboxes and changes
//! nothing else about it: the SQL core, whose mail is all the
//! deployment's own, answered `accounts=[""]` with the list itself,
//! while fastcore's per-account walk took the reader's own-only
//! conversations into the Inbox and held mail into every list.

use mailrs_core_api::client::Client;
use mailrs_core_api::method::conversation::{ListConversationsRequest, SearchConversationsRequest};
use mailrs_core_api::method::thread::MarkListReadRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::intake::{PHISH, Spools, envelope, held};
use super::search_scope::{HELLO, INVOICE, PHISHED, SALE, SENT};
use super::{ALICE, accounts, mail};

/// Ids each store mints, the action counts, which the two count
/// differently, and `trashed_at`, which each core stamps from its clock.
const MINTED: &[&str] = &[
    "id",
    "mailbox_id",
    "created_at",
    "modseq",
    "new_modseq",
    "affected",
    "trashed_at",
];

const SPAM: &str = "t-spam@spam.test";

/// Matches every conversation, so what comes back is the scope alone.
const EVERYTHING: &str = "after:2000-01-01";

fn list(f: ConversationFilter) -> ConversationFilter {
    ConversationFilter { limit: 50, ..f }
}

fn starred() -> ConversationFilter {
    list(ConversationFilter {
        folder: Some("NonJunk".into()),
        starred: Some(true),
        ..Default::default()
    })
}

fn pinned() -> ConversationFilter {
    list(ConversationFilter {
        folder: Some("NonJunk".into()),
        pinned: Some(true),
        ..Default::default()
    })
}

fn folder(name: &str) -> ConversationFilter {
    list(ConversationFilter {
        folder: Some(name.into()),
        ..Default::default()
    })
}

async fn ids(core: &Client, f: &ConversationFilter) -> Vec<String> {
    let req = ListConversationsRequest { filter: f.clone() };
    let mut ids: Vec<String> = core
        .list_conversations(ALICE, &req)
        .await
        .expect("list")
        .items
        .into_iter()
        .map(|r| r.thread_id)
        .collect();
    ids.sort();
    ids
}

async fn found(core: &Client, f: &ConversationFilter) -> Vec<String> {
    let req = SearchConversationsRequest {
        query: EVERYTHING.into(),
        filter: f.clone(),
        quarantined: false,
    };
    let mut ids: Vec<String> = core
        .search_conversations(ALICE, &req)
        .await
        .expect("search")
        .items
        .into_iter()
        .map(|r| r.thread_id)
        .collect();
    ids.sort();
    ids
}

/// The unread count All Mail shows for `tid`.
async fn unread_of(core: &Client, tid: &str) -> u32 {
    let req = ListConversationsRequest {
        filter: folder("all"),
    };
    let rows = core
        .list_conversations(ALICE, &req)
        .await
        .expect("all mail");
    rows.items
        .iter()
        .find(|r| r.thread_id == tid)
        .unwrap_or_else(|| panic!("All Mail holds {tid}"))
        .unread_count
}

fn sorted(mut v: Vec<&str>) -> Vec<String> {
    v.sort();
    v.into_iter().map(String::from).collect()
}

/// Both cores' answer to `f`, compared whole.
async fn compare(c: &Cores, d: &mut Diffs, name: &str, f: &ConversationFilter) {
    let req = ListConversationsRequest { filter: f.clone() };
    d.both(
        &format!("list_conversations [{name}]"),
        c.fastcore.list_conversations(ALICE, &req).await,
        c.pg_core.list_conversations(ALICE, &req).await,
        MINTED,
    );
}

/// Starred and pinned: her own-only conversation, an archived one, one
/// in Junk, one in the Trash and one held for review.
async fn arrange(c: &Cores) {
    Spools::new(c).arrive(c, &envelope(held()), PHISH).await;
    for core in [&c.fastcore, &c.pg_core] {
        for tid in [SENT, HELLO, SPAM, SALE, PHISHED] {
            core.star_thread(ALICE, tid).await.expect("star");
            core.pin_thread(ALICE, tid).await.expect("pin");
        }
        core.archive_thread(ALICE, HELLO).await.expect("archive");
        core.trash_thread(ALICE, SALE).await.expect("trash");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starred_and_pinned_hold_what_you_flagged_wherever_it_lives() {
    let cores = harness::start_on(
        "mem://flag-lists-fastcore-side-state",
        "mem://flag-lists-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    mail::seed(&cores, &mut d).await;
    arrange(&cores).await;
    let cores_by_name = [("fastcore", &cores.fastcore), ("pg", &cores.pg_core)];

    let mut wrong = Vec::new();
    let flagged = sorted(vec![SENT, HELLO]);
    for (name, core) in cores_by_name {
        for (list, f) in [("starred", starred()), ("pinned", pinned())] {
            let got = ids(core, &f).await;
            if got != flagged {
                wrong.push(format!("{name}: [{list}] lists {got:?}, want {flagged:?}"));
            }
            let searched = found(core, &f).await;
            if searched != got {
                wrong.push(format!(
                    "{name}: a search from [{list}] finds {searched:?}, the list shows {got:?}"
                ));
            }
        }
        // not lists of a flag: neither the own-only nor the archived one
        for (list, f) in [("inbox", folder("Inbox")), ("nonjunk", folder("NonJunk"))] {
            let got = ids(core, &f).await;
            if got.iter().any(|t| t == SENT || t == HELLO) {
                wrong.push(format!("{name}: [{list}] lists {got:?}"));
            }
        }
    }
    for (list, f) in [
        ("starred", starred()),
        ("pinned", pinned()),
        ("inbox", folder("Inbox")),
        ("nonjunk", folder("NonJunk")),
    ] {
        compare(&cores, &mut d, list, &f).await;
    }
    d.both(
        "unseen_count after flagging",
        cores.fastcore.unseen_count(ALICE).await,
        cores.pg_core.unseen_count(ALICE).await,
        &[],
    );

    // "mark all as read" in Starred reaches the archived one too
    let req = MarkListReadRequest {
        filter: starred(),
        quarantined: false,
    };
    for (name, core) in cores_by_name {
        let before = unread_of(core, HELLO).await;
        core.mark_list_conversations_read(ALICE, &req)
            .await
            .expect("mark starred read");
        let after = unread_of(core, HELLO).await;
        if before == 0 || after != 0 {
            wrong.push(format!(
                "{name}: Starred marked read, the archived starred thread went from {before} to {after} unread"
            ));
        }
        let invoice = unread_of(core, INVOICE).await;
        if invoice == 0 {
            wrong.push(format!(
                "{name}: Starred marked read reached an unstarred thread"
            ));
        }
    }
    compare(
        &cores,
        &mut d,
        "all mail after marking starred read",
        &folder("all"),
    )
    .await;

    wrong.extend(d.0);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_account_filter_narrows_a_list_and_changes_nothing_else() {
    let cores = harness::start_on(
        "mem://account-scope-fastcore-side-state",
        "mem://account-scope-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    mail::seed(&cores, &mut d).await;
    arrange(&cores).await;

    let lists = [
        ("inbox", folder("Inbox")),
        ("nonjunk", folder("NonJunk")),
        ("np", folder("NP")),
        ("junk", folder("Junk")),
        ("all mail", folder("all")),
        ("starred", starred()),
        ("pinned", pinned()),
        (
            "unread",
            list(ConversationFilter {
                folder: Some("NonJunk".into()),
                unread: Some(true),
                ..Default::default()
            }),
        ),
        (
            "archived",
            list(ConversationFilter {
                archived: true,
                ..Default::default()
            }),
        ),
        (
            "trash",
            list(ConversationFilter {
                trashed: true,
                ..Default::default()
            }),
        ),
    ];
    let mut wrong = Vec::new();
    for (name, whole) in &lists {
        let own = ConversationFilter {
            accounts: Some(vec![String::new()]),
            ..whole.clone()
        };
        let elsewhere = ConversationFilter {
            accounts: Some(vec!["acct-elsewhere".into()]),
            ..whole.clone()
        };
        for (core_name, core) in [("fastcore", &cores.fastcore), ("pg", &cores.pg_core)] {
            let want = ids(core, whole).await;
            let got = ids(core, &own).await;
            if got != want {
                wrong.push(format!(
                    "{core_name}: [{name}] of the deployment's own mail lists {got:?}, the list shows {want:?}"
                ));
            }
            let none = ids(core, &elsewhere).await;
            if !none.is_empty() {
                wrong.push(format!(
                    "{core_name}: [{name}] of a mailbox with no mail lists {none:?}"
                ));
            }
        }
        compare(&cores, &mut d, &format!("{name}, own mail only"), &own).await;
    }

    wrong.extend(d.0);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
