//! A reader's rules for one sender, on both cores: move the sender's
//! conversations, put them back, keep only the newest.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::senders::{
    ReclassifySenderRequest, RestoreSenderRequest, RetainSenderRequest,
};
use mailrs_core_api::types::ConversationFilter;

use super::ALICE;
use super::harness::{Cores, Diffs};

const FRIEND: &str = "friend@partner.test";

/// Ids, counters and file names each store mints for itself.
const MINTED: &[&str] = &[
    "id",
    "mailbox_id",
    "uid",
    "modseq",
    "created_at",
    "blob_ref",
];

fn view(folder: &str) -> ListConversationsRequest {
    ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some(folder.into()),
            limit: 50,
            ..Default::default()
        },
    }
}

async fn views(c: &Cores, d: &mut Diffs, step: &str) {
    for folder in ["Inbox", "promotions", "all"] {
        d.both(
            &format!("list_conversations [{folder}] after {step}"),
            c.fastcore.list_conversations(ALICE, &view(folder)).await,
            c.pg_core.list_conversations(ALICE, &view(folder)).await,
            MINTED,
        );
    }
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    let req = ReclassifySenderRequest {
        sender: FRIEND.into(),
        bucket: "promotions".into(),
    };
    let moved = d.both(
        "reclassify_sender",
        fc.reclassify_sender(ALICE, &req).await.map(|mut r| {
            r.moved.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
            r
        }),
        pg.reclassify_sender(ALICE, &req).await.map(|mut r| {
            r.moved.sort_by(|a, b| a.thread_id.cmp(&b.thread_id));
            r
        }),
        &[],
    );
    let moved = moved.map(|(a, _)| a.moved).unwrap_or_default();
    assert!(!moved.is_empty(), "the friend's live conversations moved");
    views(c, d, "reclassify_sender").await;

    let undo = RestoreSenderRequest {
        sender: FRIEND.into(),
        to_bucket: "promotions".into(),
        moved: moved.clone(),
    };
    let restored = d.both(
        "restore_sender",
        fc.restore_sender(ALICE, &undo).await,
        pg.restore_sender(ALICE, &undo).await,
        &[],
    );
    if let Some((r, _)) = restored {
        assert_eq!(r.restored as usize, moved.len(), "all of it went back");
    }
    views(c, d, "restore_sender").await;

    let keep_one = RetainSenderRequest {
        sender: FRIEND.into(),
        keep_latest: Some(1),
        older_than_days: None,
        disposal: "archive".into(),
        now: 1_800_000_000,
    };
    d.both(
        "retain_sender keep_latest 1",
        fc.retain_sender(ALICE, &keep_one).await,
        pg.retain_sender(ALICE, &keep_one).await,
        &[],
    );
    views(c, d, "retain_sender").await;
}
