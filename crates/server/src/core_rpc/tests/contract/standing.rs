//! A conversation's tab, its place in the list and the time it shows are
//! each reader's own, on both cores.
//!
//! The SQL core aggregates the row from the reader's own messages: the
//! category of their newest classified message (a reader's own move stamps
//! each of theirs), the newest date of mail they received. fastcore copied
//! the shared thread hash's category and date, whichever member's mail
//! came or whichever member filed it last, onto a member's row whenever
//! that row was rewritten, so Alice's move put Bob's conversation in his
//! Promotions tab, and Dave's reply, which only Alice received, moved
//! Bob's conversation up his list with Dave's time on it.

use mailrs_core::spool::SpoolEnvelope;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};

use super::harness::{self, Cores, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, accounts};

const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

const CAROLS: &[u8] = b"From: carol@outside.example\r\n\
To: alice@example.test, bob@example.test\r\n\
Subject: Friday agenda\r\n\
Message-ID: <std1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
Here is the agenda for Friday.\r\n";

const LUNCH: &[u8] = b"From: erin@outside.example\r\n\
To: bob@example.test\r\n\
Subject: Lunch\r\n\
Message-ID: <std3@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:30:00 +0900\r\n\
\r\n\
Lunch on Thursday?\r\n";

const DAVES: &[u8] = b"From: Dave Zimmerman <dave@outside.example>\r\n\
To: alice@example.test\r\n\
Subject: Re: Friday agenda\r\n\
Message-ID: <std2@outside.example>\r\n\
In-Reply-To: <std1@outside.example>\r\n\
References: <std1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 10:00:00 +0900\r\n\
\r\n\
Can we move it to Monday?\r\n";

const BOBS_REPLY: &[u8] = b"From: Bob <bob@example.test>\r\n\
To: carol@outside.example\r\n\
Subject: Re: Friday agenda\r\n\
Message-ID: <std4@example.test>\r\n\
In-Reply-To: <std1@outside.example>\r\n\
References: <std1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:40:00 +0900\r\n\
\r\n\
Thanks, see you there.\r\n";

const TID: &str = "std1@outside.example";
const LUNCH_TID: &str = "std3@outside.example";
/// Carol's message, 09:00 JST.
const CAROLS_AT: i64 = 1_754_352_000;

fn folder(name: &str) -> ListConversationsRequest {
    ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            folder: Some(name.into()),
            ..Default::default()
        },
    }
}

/// `user`'s list in `name`, per core, compared.
async fn list(
    c: &Cores,
    d: &mut Diffs,
    user: &str,
    name: &str,
    step: &str,
) -> [Vec<ConversationSummaryWire>; 2] {
    let req = folder(name);
    let (fc, pg) = d
        .both(
            &format!("list_conversations [{name}] for {user} after {step}"),
            c.fastcore.list_conversations(user, &req).await,
            c.pg_core.list_conversations(user, &req).await,
            MINTED,
        )
        .unwrap_or_else(|| panic!("both cores list {user}'s {name}"));
    [fc.items, pg.items]
}

/// Bob's Inbox holds Lunch above the shared conversation at Carol's time,
/// nothing of it is in his other tabs, and his counts agree on both cores.
async fn bob_untouched(c: &Cores, d: &mut Diffs, step: &str) {
    for (core, items) in ["fastcore", "pg-core"]
        .into_iter()
        .zip(list(c, d, BOB, "Inbox", step).await)
    {
        let order: Vec<&str> = items.iter().map(|r| r.thread_id.as_str()).collect();
        assert_eq!(order, [LUNCH_TID, TID], "{core} after {step}: Bob's Inbox");
        let row = &items[1];
        assert_eq!(row.last_date, CAROLS_AT, "{core} after {step}: Bob's time");
        assert_eq!(row.category, "inbox", "{core} after {step}: Bob's category");
    }
    for (core, items) in ["fastcore", "pg-core"]
        .into_iter()
        .zip(list(c, d, BOB, "NP", step).await)
    {
        assert!(items.is_empty(), "{core} after {step}: Bob's NP {items:?}");
    }
    d.both(
        &format!("conversation_categories for bob after {step}"),
        c.fastcore.conversation_categories(BOB).await,
        c.pg_core.conversation_categories(BOB).await,
        &[],
    );
    d.both(
        &format!("unseen_count for bob after {step}"),
        c.fastcore.unseen_count(BOB).await,
        c.pg_core.unseen_count(BOB).await,
        &[],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_readers_mail_and_moves_leave_the_others_tab_and_time() {
    let cores = harness::start_on(
        "mem://standing-fastcore-side-state",
        "mem://standing-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    let to_both = SpoolEnvelope {
        forward_paths: vec![ALICE.into(), BOB.into()],
        ..envelope(None)
    };
    spools.arrive(&cores, &to_both, CAROLS).await;
    let to_bob = SpoolEnvelope {
        forward_paths: vec![BOB.into()],
        ..envelope(None)
    };
    spools.arrive(&cores, &to_bob, LUNCH).await;
    let to_alice = SpoolEnvelope {
        reverse_path: "dave@outside.example".into(),
        ..envelope(None)
    };
    spools.arrive(&cores, &to_alice, DAVES).await;
    bob_untouched(&cores, &mut d, "Dave's reply to Alice").await;

    // Bob's own reply rewrites his row from what the core holds
    let rcpts = vec!["carol@outside.example".to_string()];
    d.both(
        "file_sent bob's reply",
        cores.fastcore.file_sent(BOB, BOBS_REPLY, &rcpts).await,
        cores.pg_core.file_sent(BOB, BOBS_REPLY, &rcpts).await,
        &["blob_ref"],
    );
    bob_untouched(&cores, &mut d, "Bob's reply").await;

    d.both(
        "mark_promotion by alice",
        cores.fastcore.mark_promotion(ALICE, TID).await.map(|_| ()),
        cores.pg_core.mark_promotion(ALICE, TID).await.map(|_| ()),
        &[],
    );
    for (core, items) in ["fastcore", "pg-core"]
        .into_iter()
        .zip(list(&cores, &mut d, ALICE, "NP", "Alice's move").await)
    {
        assert_eq!(items.len(), 1, "{core}: Alice's move is hers");
    }
    bob_untouched(&cores, &mut d, "Alice's move").await;

    d.both(
        "archive_thread by alice",
        cores.fastcore.archive_thread(ALICE, TID).await.map(|_| ()),
        cores.pg_core.archive_thread(ALICE, TID).await.map(|_| ()),
        &[],
    );
    bob_untouched(&cores, &mut d, "Alice's archive").await;
    d.both(
        "delete_thread by alice",
        cores.fastcore.delete_thread(ALICE, TID).await,
        cores.pg_core.delete_thread(ALICE, TID).await,
        &[],
    );
    bob_untouched(&cores, &mut d, "Alice's delete").await;

    // and Bob's own move is his
    d.both(
        "mark_promotion by bob",
        cores.fastcore.mark_promotion(BOB, TID).await.map(|_| ()),
        cores.pg_core.mark_promotion(BOB, TID).await.map(|_| ()),
        &[],
    );
    for (core, items) in ["fastcore", "pg-core"]
        .into_iter()
        .zip(list(&cores, &mut d, BOB, "NP", "Bob's move").await)
    {
        assert_eq!(items.len(), 1, "{core}: Bob's move");
        assert_eq!(items[0].last_date, CAROLS_AT, "{core}: Bob's time");
    }

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
