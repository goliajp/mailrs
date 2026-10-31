//! The Trash answers alike on both cores: moving a conversation there
//! takes it out of the ordinary lists and into the Trash, restoring brings
//! it back, and emptying deletes it for good.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, accounts};

fn mail(id: &str) -> Vec<u8> {
    format!(
        "From: Carol <carol@outside.example>\r\nTo: alice@example.test\r\n\
         Subject: {id}\r\nMessage-ID: <{id}@outside.example>\r\n\
         Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\r\nhello\r\n"
    )
    .into_bytes()
}

async fn lists(c: &Cores, d: &mut Diffs, step: &str) -> (Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    for trashed in [false, true] {
        let req = ListConversationsRequest {
            filter: ConversationFilter {
                limit: 50,
                trashed,
                ..Default::default()
            },
        };
        let got = d.both(
            &format!("list_conversations trashed={trashed} after {step}"),
            c.fastcore.list_conversations(ALICE, &req).await,
            c.pg_core.list_conversations(ALICE, &req).await,
            &["id", "mailbox_id", "created_at", "modseq"],
        );
        let mut ids: Vec<String> = got
            .map(|(fc, _)| fc.items.into_iter().map(|r| r.thread_id).collect())
            .unwrap_or_default();
        ids.sort();
        out.push(ids);
    }
    (out.remove(0), out.remove(0))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_trash_answers_alike_on_both_cores() {
    let cores = harness::start_on(
        "mem://trash-fastcore-side-state",
        "mem://trash-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    for id in ["t1", "t2", "t3"] {
        spools.arrive(&cores, &envelope(None), &mail(id)).await;
    }
    let tid = |id: &str| format!("{id}@outside.example");

    for c in [&cores.fastcore, &cores.pg_core] {
        c.trash_thread(ALICE, &tid("t1")).await.expect("trash");
        c.trash_thread(ALICE, &tid("t2")).await.expect("trash");
    }
    let (live, trash) = lists(&cores, &mut d, "trash").await;
    assert_eq!(live, [tid("t3")]);
    assert_eq!(trash, [tid("t1"), tid("t2")]);

    for c in [&cores.fastcore, &cores.pg_core] {
        c.untrash_thread(ALICE, &tid("t1")).await.expect("untrash");
    }
    let (live, trash) = lists(&cores, &mut d, "untrash").await;
    assert_eq!(live, [tid("t1"), tid("t3")]);
    assert_eq!(trash, [tid("t2")]);

    d.both(
        "empty_trash",
        cores.fastcore.empty_trash(ALICE).await,
        cores.pg_core.empty_trash(ALICE).await,
        &[],
    );
    let (live, trash) = lists(&cores, &mut d, "empty").await;
    assert_eq!(live, [tid("t1"), tid("t3")]);
    assert!(trash.is_empty());
    assert!(d.0.is_empty(), "the cores differ:\n{}", d.0.join("\n"));
}
