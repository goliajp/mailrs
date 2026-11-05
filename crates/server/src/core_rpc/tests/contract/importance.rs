//! The importance verdict is one reader's, on both cores.
//!
//! The SQL core keeps it on each user's own copy of a message, so a member
//! who joins a conversation later starts unscored. fastcore kept it on the
//! shared thread hash as well and copied it onto the next member's row: Bob,
//! whose only message in Alice's conversation is his own reply, read Alice's
//! score.

use mailrs_core::spool::SpoolEnvelope;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, accounts};

const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

const ASK: &[u8] = b"From: Carol <carol@outside.example>\r\n\
To: alice@example.test\r\n\
Subject: Contract renewal\r\n\
Message-ID: <imp1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
Can you confirm the renewal terms by Monday?\r\n";

const BOBS_REPLY: &[u8] = b"From: Bob <bob@example.test>\r\n\
To: alice@example.test\r\n\
Subject: Re: Contract renewal\r\n\
Message-ID: <imp2@example.test>\r\n\
In-Reply-To: <imp1@outside.example>\r\n\
References: <imp1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 10:00:00 +0900\r\n\
\r\n\
I can take this one.\r\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_later_member_does_not_read_an_earlier_members_importance() {
    let cores = harness::start_on(
        "mem://importance-fastcore-side-state",
        "mem://importance-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);

    spools.arrive(&cores, &envelope(None), ASK).await;
    let from_bob = SpoolEnvelope {
        reverse_path: BOB.into(),
        forward_paths: vec![ALICE.into(), BOB.into()],
        ..envelope(None)
    };
    spools.arrive(&cores, &from_bob, BOBS_REPLY).await;

    let sent = ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some("Sent".into()),
            limit: 50,
            ..Default::default()
        },
    };
    // `participants` is left out, and not because it may differ: fastcore's
    // comes from the shared thread hash and names Carol, who never wrote to
    // Bob. The same leak in another field, and a defect of its own.
    let ignore: Vec<&str> = MINTED.iter().copied().chain(["participants"]).collect();
    let rows = d.both(
        "list_conversations [Sent] for the member who only replied",
        cores.fastcore.list_conversations(BOB, &sent).await,
        cores.pg_core.list_conversations(BOB, &sent).await,
        &ignore,
    );
    let (fc, pg) = rows.expect("both cores list Bob's Sent");
    for (core, rows) in [("fastcore", &fc), ("pg-core", &pg)] {
        let row = rows
            .items
            .iter()
            .find(|r| r.thread_id == "imp1@outside.example")
            .unwrap_or_else(|| panic!("{core}: Bob's reply is not in his Sent"));
        assert_eq!(
            (row.importance_level.as_str(), row.importance_score),
            ("", 0.0),
            "{core}: Bob reads a score he was never given"
        );
    }

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
