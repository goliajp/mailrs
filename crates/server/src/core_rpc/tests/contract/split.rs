//! An operator moves a reply out of the conversation it was joined to,
//! alike on both cores.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Diffs};
use super::{ALICE, BOB, accounts, mail};

/// Ids, counters and clocks each store keeps for itself.
const MINTED: &[&str] = &["id", "mailbox_id", "uid", "modseq", "cursor"];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_split_reply_stands_alone_on_both_cores() {
    let c = harness::start_on("mem://split-fastcore", "mem://split-pg-core").await;
    let mut d = Diffs::default();
    accounts::seed(&c, &mut d).await;
    mail::seed(&c, &mut d).await;

    let split = d.both(
        "split_message",
        c.fastcore.split_message(ALICE, "m2@partner.test").await,
        c.pg_core.split_message(ALICE, "m2@partner.test").await,
        &[],
    );
    if let Some((_, pg)) = &split {
        assert_eq!(pg.thread_id, "m2@partner.test");
    }
    for tid in ["t-hello@partner.test", "m2@partner.test"] {
        d.both(
            &format!("list_thread_messages {tid} after the split"),
            c.fastcore.list_thread_messages(ALICE, tid).await,
            c.pg_core.list_thread_messages(ALICE, tid).await,
            MINTED,
        );
    }
    let req = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            ..Default::default()
        },
    };
    // Known difference: the kevy store keeps a preview and a section per
    // conversation, not per message, so after a split it cannot say what
    // the moved reply's line was nor the line left behind; the SQL store
    // recomputes both from the messages. The next arrival in either
    // conversation sets them right.
    let display: Vec<&str> = MINTED
        .iter()
        .copied()
        .chain(["snippet", "section"])
        .collect();
    d.both(
        "list_conversations after the split",
        c.fastcore.list_conversations(ALICE, &req).await,
        c.pg_core.list_conversations(ALICE, &req).await,
        &display,
    );
    for (who, mid) in [(BOB, "m1@partner.test"), (ALICE, "nothing@nowhere.test")] {
        d.both(
            &format!("split_message {mid} as {who}"),
            c.fastcore.split_message(who, mid).await,
            c.pg_core.split_message(who, mid).await,
            &[],
        );
    }
    d.0.extend(super::coverage::require("split", &c));
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
}
