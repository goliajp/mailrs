//! The Trash answers alike on both cores: moving a conversation there
//! takes it out of the ordinary lists and into the Trash, restoring brings
//! it back, emptying deletes it for good, and a reply from somebody else
//! brings a conversation out of the Trash, as Gmail does, rather than
//! leaving it for the purge to delete unread.

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
            // each core stamps trashed_at from its own clock, so the two
            // can straddle a second; what must agree is that it is set
            &["id", "mailbox_id", "created_at", "modseq", "trashed_at"],
        );
        let mut ids: Vec<String> = got
            .map(|(fc, pg)| {
                for row in fc.items.iter().chain(pg.items.iter()) {
                    assert_eq!(
                        row.trashed_at > 0,
                        trashed,
                        "trashed_at on {} after {step}",
                        row.thread_id
                    );
                }
                fc.items.into_iter().map(|r| r.thread_id).collect()
            })
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

    for c in [&cores.fastcore, &cores.pg_core] {
        c.trash_thread(ALICE, &tid("t3")).await.expect("trash");
    }
    let reply = "From: Carol <carol@outside.example>\r\nTo: alice@example.test\r\n\
         Subject: Re: t3\r\nMessage-ID: <t3-reply@outside.example>\r\n\
         In-Reply-To: <t3@outside.example>\r\nReferences: <t3@outside.example>\r\n\
         Date: Wed, 6 Aug 2025 09:00:00 +0900\r\n\r\nstill there?\r\n";
    spools
        .arrive(&cores, &envelope(None), reply.as_bytes())
        .await;
    let (live, trash) = lists(&cores, &mut d, "a reply to a trashed one").await;
    assert_eq!(live, [tid("t1"), tid("t3")]);
    assert!(trash.is_empty(), "{trash:?}");
    assert!(d.0.is_empty(), "the cores differ:\n{}", d.0.join("\n"));
}

/// `ALICE`'s used bytes on the network kevy at `url`, 0 when unset.
fn used(url: &str) -> i64 {
    let mut c = kevy_client::Connection::connect(url).expect("kevy");
    let key = mailrs_core_sidestate::keys::quota::used_key(ALICE);
    c.get(key.as_bytes())
        .expect("read used bytes")
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Mail that arrives, goes to the Trash and is deleted for good there,
/// by the purge or by emptying the Trash, leaves the account's used bytes
/// where they were before it came: the quota stage reads that counter,
/// and one that only grows refuses mail to an account that has room.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mail_deleted_from_the_trash_leaves_the_usage_counter_as_it_was() {
    let urls = [
        "mem://usage-fastcore-side-state",
        "mem://usage-pg-core-side-state",
    ];
    let cores = harness::start_on(urls[0], urls[1]).await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let before = urls.map(used);
    let spools = Spools::new(&cores);
    for id in ["u1", "u2"] {
        spools.arrive(&cores, &envelope(None), &mail(id)).await;
    }
    let tid = |id: &str| format!("{id}@outside.example");
    let after = urls.map(used);
    for (core, (b, a)) in ["fastcore", "pg-core"].iter().zip(before.iter().zip(after)) {
        assert!(a > *b, "{core}: arriving mail counts ({b} -> {a})");
    }

    for c in [&cores.fastcore, &cores.pg_core] {
        c.trash_thread(ALICE, &tid("u1")).await.expect("trash");
        c.trash_thread(ALICE, &tid("u2")).await.expect("trash");
    }
    assert_eq!(urls.map(used), after, "the Trash still counts");

    // u1 by the purge, u2 by emptying the Trash
    cores
        .fastcore_state
        .mailbox
        .set_trashed(ALICE, &tid("u1"), true, 1_000)
        .expect("backdate");
    cores
        .pg_core_state
        .mailbox
        .trash_thread(ALICE, &tid("u1"), 1_000)
        .await
        .expect("backdate");
    let fc = mailrs_fastcore::purge_trash_once(&cores.fastcore_state, 2_000, 10).await;
    let pg = crate::core_rpc::handlers::trash::purge_once(&cores.pg_core_state, 2_000, 10).await;
    assert_eq!((fc, pg), (1, 1), "the purge deletes u1 on each core");

    let emptied = d.both(
        "empty_trash",
        cores.fastcore.empty_trash(ALICE).await,
        cores.pg_core.empty_trash(ALICE).await,
        &[],
    );
    if let Some((e, _)) = emptied {
        assert_eq!(e.deleted.len(), 1, "{e:?}");
        assert_eq!(e.deleted[0].thread_id, tid("u2"));
        assert_eq!(e.deleted[0].messages, 1);
    }
    assert_eq!(urls.map(used), before, "the counter is where it started");
    assert!(d.0.is_empty(), "the cores differ:\n{}", d.0.join("\n"));
}
