//! A conversation's participants are each reader's own, on both cores.
//!
//! The SQL core aggregates them from the reader's own messages. fastcore
//! copied the shared thread hash's list, every sender any member received,
//! onto each member's row: Bob, who joined Alice's conversation with Dave's
//! message, was shown Carol, who only ever wrote to Alice.

use mailrs_core::spool::SpoolEnvelope;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, accounts};

const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

const CAROLS: &[u8] = b"From: carol@outside.example\r\n\
To: alice@example.test\r\n\
Subject: Venue\r\n\
Message-ID: <par1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
Which room are we in?\r\n";

const DAVES: &[u8] = b"From: dave@outside.example\r\n\
To: alice@example.test, bob@example.test\r\n\
Subject: Re: Venue\r\n\
Message-ID: <par2@outside.example>\r\n\
In-Reply-To: <par1@outside.example>\r\n\
References: <par1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 10:00:00 +0900\r\n\
\r\n\
Room 4, and I've asked Bob along.\r\n";

const TID: &str = "par1@outside.example";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_later_member_does_not_see_an_earlier_participant() {
    let cores = harness::start_on(
        "mem://participants-fastcore-side-state",
        "mem://participants-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);

    spools.arrive(&cores, &envelope(None), CAROLS).await;
    let to_both = SpoolEnvelope {
        reverse_path: "dave@outside.example".into(),
        forward_paths: vec![ALICE.into(), BOB.into()],
        ..envelope(None)
    };
    spools.arrive(&cores, &to_both, DAVES).await;

    let inbox = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            ..Default::default()
        },
    };
    for (user, want) in [
        (BOB, "dave@outside.example"),
        (ALICE, "carol@outside.example,dave@outside.example"),
    ] {
        let rows = d.both(
            &format!("list_conversations for {user}"),
            cores.fastcore.list_conversations(user, &inbox).await,
            cores.pg_core.list_conversations(user, &inbox).await,
            MINTED,
        );
        let (fc, pg) = rows.unwrap_or_else(|| panic!("both cores list {user}'s mail"));
        for (core, rows) in [("fastcore", &fc), ("pg-core", &pg)] {
            let row = rows
                .items
                .iter()
                .find(|r| r.thread_id == TID)
                .unwrap_or_else(|| panic!("{core}: {user} does not hold the conversation"));
            assert_eq!(row.participants, want, "{core}: {user}'s participants");
        }
    }

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
