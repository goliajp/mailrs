//! A held conversation keeps its unread mail on both cores: out of the
//! badge and out of "mark all as read" while it is held, and unread again
//! in the inbox once it is released.

use mailrs_core::spool::SpoolEnvelope;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::intake::{PHISH, REPORT, Spools, envelope, held};
use super::{ALICE, accounts};

/// Ids and counters each store keeps for itself.
const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

const TID: &str = "p1@outside.example";

const FOLLOW_UP: &[u8] = b"From: Bank <alerts@outside.example>\r\n\
To: alice@example.test\r\n\
Subject: Re: Your account is locked\r\n\
Message-ID: <p2@outside.example>\r\n\
In-Reply-To: <p1@outside.example>\r\n\
References: <p1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 13:00:00 +0900\r\n\
\r\n\
Last reminder.\r\n";

/// `(count, inbox)` of the badge on each core, compared.
async fn badge(c: &Cores, d: &mut Diffs, step: &str) -> [(i64, i64); 2] {
    let (fc, pg) = d
        .both(
            &format!("unseen_count {step}"),
            c.fastcore.unseen_count(ALICE).await,
            c.pg_core.unseen_count(ALICE).await,
            &[],
        )
        .expect("both cores count");
    [(fc.count, fc.inbox), (pg.count, pg.inbox)]
}

/// The held conversation's unread count in Review on each core, compared.
async fn held_unread(c: &Cores, d: &mut Diffs, step: &str) -> [u32; 2] {
    let (fc, pg) = d
        .both(
            &format!("list_quarantined {step}"),
            c.fastcore.list_quarantined(ALICE, 50, None).await,
            c.pg_core.list_quarantined(ALICE, 50, None).await,
            MINTED,
        )
        .expect("both cores list what they hold");
    let of = |items: &[mailrs_core_api::types::ConversationSummaryWire]| {
        items
            .iter()
            .find(|r| r.thread_id == TID)
            .map(|r| r.unread_count)
            .expect("the held conversation is in Review")
    };
    [of(&fc.items), of(&pg.items)]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_conversation_keeps_its_unread_mail_on_both_cores() {
    let cores = harness::start_on(
        "mem://held-read-fastcore-side-state",
        "mem://held-read-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    let plain: SpoolEnvelope = envelope(None);

    spools.arrive(&cores, &plain, REPORT).await;
    spools.arrive(&cores, &envelope(held()), PHISH).await;
    assert_eq!(
        badge(&cores, &mut d, "after a hold").await,
        [(1, 1); 2],
        "the badge counted held mail"
    );
    assert_eq!(
        held_unread(&cores, &mut d, "after a hold").await,
        [1; 2],
        "held and marked read"
    );

    // mail with no verdict arriving while it is held is unread and stays
    // out of the badge
    spools.arrive(&cores, &plain, FOLLOW_UP).await;
    assert_eq!(
        badge(&cores, &mut d, "after mail into a held conversation").await,
        [(1, 1); 2],
        "the badge counted mail in a held conversation"
    );
    assert_eq!(
        held_unread(&cores, &mut d, "after mail into a held conversation").await,
        [2; 2]
    );

    let flipped = d.both(
        "mark_all_conversations_read beside a held conversation",
        cores.fastcore.mark_all_conversations_read(ALICE).await,
        cores.pg_core.mark_all_conversations_read(ALICE).await,
        &[],
    );
    assert_eq!(flipped, Some((1, 1)), "mark all read took held mail");
    assert_eq!(
        held_unread(&cores, &mut d, "after mark all read").await,
        [2; 2],
        "mark all read touched held mail"
    );
    assert_eq!(
        badge(&cores, &mut d, "after mark all read").await,
        [(0, 0); 2]
    );

    for c in [&cores.fastcore, &cores.pg_core] {
        c.release_quarantined(ALICE, TID).await.expect("release");
    }
    let req = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            ..Default::default()
        },
    };
    let rows = d
        .both(
            "list_conversations after a release",
            cores.fastcore.list_conversations(ALICE, &req).await,
            cores.pg_core.list_conversations(ALICE, &req).await,
            MINTED,
        )
        .expect("both cores list");
    for (core, items) in [("fastcore", &rows.0.items), ("pg-core", &rows.1.items)] {
        let row = items
            .iter()
            .find(|r| r.thread_id == TID)
            .unwrap_or_else(|| panic!("{core}: the released conversation is not listed"));
        assert_eq!(row.unread_count, 2, "{core}: released read");
    }
    assert_eq!(
        badge(&cores, &mut d, "after a release").await,
        [(1, 1); 2],
        "released, and not in the badge"
    );

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
