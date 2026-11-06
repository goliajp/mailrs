//! "Mark all as read" pressed in a list marks what that list shows, on
//! both cores, for the reader who pressed it: no conversation outside the
//! list, none of another reader's copies, and every message of each one
//! marked, not only the conversation's counter.

use std::collections::BTreeSet;

use mailrs_core_api::client::Client;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::message::FLAG_SEEN;
use mailrs_core_api::method::thread::MarkListReadRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::search_scope::{
    HELLO, INTRO, INVOICE, PHISHED, SALE, SENT, SHARED, TEAM, arrange, lists,
};
use super::{ALICE, BOB, accounts, mail};

const FC_KEVY: &str = "mem://mark-list-read-fastcore-side-state";
const PG_KEVY: &str = "mem://mark-list-read-pg-core-side-state";

/// Every conversation `user` has, wherever it is, and whether it is
/// unread: All Mail and Junk with held mail shown in place, the Trash,
/// and Review.
async fn unread_state(core: &Client, user: &str) -> Vec<(String, bool)> {
    let places = [
        ConversationFilter {
            folder: Some("all".into()),
            include_quarantined: true,
            ..Default::default()
        },
        ConversationFilter {
            folder: Some("Junk".into()),
            include_quarantined: true,
            ..Default::default()
        },
        ConversationFilter {
            trashed: true,
            ..Default::default()
        },
    ];
    let mut rows = Vec::new();
    for f in places {
        let req = ListConversationsRequest {
            filter: ConversationFilter { limit: 1000, ..f },
        };
        rows.extend(
            core.list_conversations(user, &req)
                .await
                .expect("list")
                .items,
        );
    }
    rows.extend(
        core.list_quarantined(user, 1000, None)
            .await
            .expect("review")
            .items,
    );
    let mut state: Vec<(String, bool)> = rows
        .into_iter()
        .map(|r| (r.thread_id, r.unread_count > 0))
        .collect();
    state.sort();
    state.dedup();
    state
}

/// The conversations the list shows to `user`.
async fn shown(core: &Client, user: &str, req: &MarkListReadRequest) -> BTreeSet<String> {
    let items = match req.quarantined {
        true => {
            core.list_quarantined(user, 1000, None)
                .await
                .expect("review")
                .items
        }
        false => {
            let list = ListConversationsRequest {
                filter: ConversationFilter {
                    limit: 1000,
                    ..req.filter.clone()
                },
            };
            core.list_conversations(user, &list)
                .await
                .expect("list")
                .items
        }
    };
    items.into_iter().map(|r| r.thread_id).collect()
}

/// Every conversation of both readers unread again, on both cores.
async fn all_unread(c: &Cores) {
    for core in [&c.fastcore, &c.pg_core] {
        for user in [ALICE, BOB] {
            for (tid, _) in unread_state(core, user).await {
                core.mark_thread_unread(user, &tid).await.expect("unread");
            }
        }
    }
}

fn unread_ids(state: &[(String, bool)]) -> BTreeSet<String> {
    state
        .iter()
        .filter(|(_, unread)| *unread)
        .map(|(tid, _)| tid.clone())
        .collect()
}

/// The lists a client has a "mark all as read" in.
fn marked_lists() -> Vec<(&'static str, MarkListReadRequest)> {
    let mut out: Vec<(&'static str, MarkListReadRequest)> = lists()
        .into_iter()
        .map(|(name, filter)| {
            (
                name,
                MarkListReadRequest {
                    filter,
                    quarantined: false,
                },
            )
        })
        .collect();
    out.push((
        "unread",
        MarkListReadRequest {
            filter: ConversationFilter {
                folder: Some("NonJunk".into()),
                unread: Some(true),
                limit: 50,
                ..Default::default()
            },
            quarantined: false,
        },
    ));
    out.push((
        "review",
        MarkListReadRequest {
            filter: ConversationFilter::default(),
            quarantined: true,
        },
    ));
    out
}

/// One reader marking one list on one core: what was unread before and
/// what the call marked, with what went wrong pushed onto `wrong`.
async fn check_one(
    core: &Client,
    core_name: &str,
    user: &str,
    name: &str,
    req: &MarkListReadRequest,
    wrong: &mut Vec<String>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let other = match user {
        ALICE => BOB,
        _ => ALICE,
    };
    let before = unread_state(core, user).await;
    let others_before = unread_state(core, other).await;
    let listed = shown(core, user, req).await;
    let want: BTreeSet<String> = unread_ids(&before).intersection(&listed).cloned().collect();

    let flipped = core
        .mark_list_conversations_read(user, req)
        .await
        .expect("mark list read");

    let after = unread_state(core, user).await;
    let marked: BTreeSet<String> = unread_ids(&before)
        .difference(&unread_ids(&after))
        .cloned()
        .collect();
    let at = format!("{core_name}: mark all read in [{name}] as {user}");
    if marked != want {
        wrong.push(format!(
            "{at} marked {marked:?}, the list shows unread {want:?}"
        ));
    }
    if flipped as usize != marked.len() {
        wrong.push(format!("{at} said {flipped}, marked {}", marked.len()));
    }
    if unread_state(core, other).await != others_before {
        wrong.push(format!("{at} changed {other}'s copies"));
    }
    for tid in &marked {
        let messages = core
            .list_thread_messages(user, tid)
            .await
            .expect("messages")
            .items;
        if messages.iter().any(|m| m.flags & FLAG_SEEN == 0) {
            wrong.push(format!("{at}: {tid} reads as read, a message of it is not"));
        }
    }
    (unread_ids(&before), marked)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mark_all_read_marks_what_its_list_shows_on_both_cores() {
    let cores = harness::start_on(FC_KEVY, PG_KEVY).await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    mail::seed(&cores, &mut d).await;
    arrange(&cores, [FC_KEVY, PG_KEVY]).await;

    // the places read for the state hold every conversation arranged
    all_unread(&cores).await;
    for (core, client) in [("fastcore", &cores.fastcore), ("pg", &cores.pg_core)] {
        let held = unread_ids(&unread_state(client, ALICE).await);
        for tid in [HELLO, INVOICE, SALE, SENT, SHARED, INTRO, PHISHED, TEAM] {
            assert!(
                held.contains(tid),
                "{core}: {tid} is not unread for {ALICE}: {held:?}"
            );
        }
    }

    // every list checked before failing, so one run names every scope
    let mut wrong = Vec::new();
    for user in [ALICE, BOB] {
        for (name, req) in marked_lists() {
            all_unread(&cores).await;
            let (fc_unread, fc) =
                check_one(&cores.fastcore, "fastcore", user, name, &req, &mut wrong).await;
            let (pg_unread, pg) =
                check_one(&cores.pg_core, "pg", user, name, &req, &mut wrong).await;
            if fc_unread != pg_unread {
                wrong.push(format!(
                    "[{name}] as {user}: unread before, fastcore {fc_unread:?}, pg {pg_unread:?}"
                ));
            }
            if fc != pg {
                wrong.push(format!(
                    "[{name}] as {user}: fastcore marked {fc:?}, pg {pg:?}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    // what each list marks, so two cores marking nothing cannot agree
    let expect: &[(&str, &str, &[&str])] = &[
        (ALICE, "pinned", &[HELLO]),
        (BOB, "pinned", &[SHARED]),
        (ALICE, "snoozed", &[INVOICE]),
        (ALICE, "trash", &[SALE]),
        (ALICE, "label", &[INTRO]),
        (BOB, "assigned to me", &[TEAM]),
        (ALICE, "review", &[PHISHED]),
    ];
    for (user, name, ids) in expect {
        let (_, req) = marked_lists()
            .into_iter()
            .find(|(n, _)| n == name)
            .expect("a list of that name");
        for (core, client) in [("fastcore", &cores.fastcore), ("pg", &cores.pg_core)] {
            all_unread(&cores).await;
            let before = unread_ids(&unread_state(client, user).await);
            client
                .mark_list_conversations_read(user, &req)
                .await
                .expect("mark list read");
            let after = unread_ids(&unread_state(client, user).await);
            let read: BTreeSet<String> = before.difference(&after).cloned().collect();
            let want: BTreeSet<String> = ids.iter().map(|s| s.to_string()).collect();
            assert_eq!(read, want, "{core}: [{name}] as {user}");
        }
    }
}
