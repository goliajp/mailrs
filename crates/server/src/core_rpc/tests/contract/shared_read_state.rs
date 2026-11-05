//! One member reading a conversation leaves the other member's copy
//! unread, on both cores: their list row, their badge and their message.
//!
//! fastcore's `mark_seen` also set the read bit on the shared blob of every
//! message in the conversation, which a later split of the other member's
//! copy read back as theirs. The SQL core keeps read state on each
//! reader's own message rows only.

use mailrs_core::spool::SpoolEnvelope;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, accounts};

const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

const TO_BOTH: &[u8] = b"From: carol@outside.example\r\n\
To: alice@example.test, bob@example.test\r\n\
Subject: Agenda\r\n\
Message-ID: <rs1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
Here is the agenda for Friday.\r\n";

const TID: &str = "rs1@outside.example";

/// `user`'s unread count on the conversation and their badge, per core,
/// compared.
async fn unread(c: &Cores, d: &mut Diffs, user: &str, step: &str) -> [(u32, i64); 2] {
    let inbox = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            ..Default::default()
        },
    };
    let (fc, pg) = d
        .both(
            &format!("list_conversations for {user} {step}"),
            c.fastcore.list_conversations(user, &inbox).await,
            c.pg_core.list_conversations(user, &inbox).await,
            MINTED,
        )
        .unwrap_or_else(|| panic!("both cores list {user}'s mail"));
    let (bfc, bpg) = d
        .both(
            &format!("unseen_count for {user} {step}"),
            c.fastcore.unseen_count(user).await,
            c.pg_core.unseen_count(user).await,
            &[],
        )
        .expect("both cores count");
    let row = |items: &[mailrs_core_api::types::ConversationSummaryWire]| {
        items
            .iter()
            .find(|r| r.thread_id == TID)
            .map(|r| r.unread_count)
            .unwrap_or_else(|| panic!("{user} holds the conversation"))
    };
    [(row(&fc.items), bfc.count), (row(&pg.items), bpg.count)]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_members_read_is_not_anothers_on_both_cores() {
    let cores = harness::start_on(
        "mem://shared-read-fastcore-side-state",
        "mem://shared-read-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    let to_both = SpoolEnvelope {
        forward_paths: vec![ALICE.into(), BOB.into()],
        ..envelope(None)
    };
    spools.arrive(&cores, &to_both, TO_BOTH).await;

    for core in [&cores.fastcore, &cores.pg_core] {
        core.mark_thread_read(ALICE, TID)
            .await
            .expect("Alice reads it");
    }
    assert_eq!(
        unread(&cores, &mut d, ALICE, "after her read").await,
        [(0, 0); 2]
    );
    assert_eq!(
        unread(&cores, &mut d, BOB, "after Alice's read").await,
        [(1, 1); 2],
        "Alice's read reached Bob's copy"
    );

    for core in [&cores.fastcore, &cores.pg_core] {
        core.mark_thread_read(BOB, TID).await.expect("Bob reads it");
    }
    assert_eq!(
        unread(&cores, &mut d, BOB, "after his read").await,
        [(0, 0); 2]
    );

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
