//! A conversation's subject, preview and search are each reader's own, on
//! both cores.
//!
//! The SQL core aggregates the row from the reader's own messages and
//! searches them. fastcore copied the shared thread hash's subject and
//! preview, whichever member's mail arrived last, onto each member's row,
//! and searched a text field on that hash built from every member's mail:
//! Bob, who never received Dave's reply to Alice, could read its opening
//! line and find the conversation by Dave's name.

use mailrs_core::spool::SpoolEnvelope;
use mailrs_core_api::method::conversation::{ListConversationsRequest, SearchConversationsRequest};
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Cores, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, accounts};

const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

const CAROLS: &[u8] = b"From: carol@outside.example\r\n\
To: alice@example.test, bob@example.test\r\n\
Subject: Friday agenda\r\n\
Message-ID: <dsp1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
Here is the agenda for Friday.\r\n";

const DAVES: &[u8] = b"From: Dave Zimmerman <dave@outside.example>\r\n\
To: alice@example.test\r\n\
Subject: Re: Friday agenda\r\n\
Message-ID: <dsp2@outside.example>\r\n\
In-Reply-To: <dsp1@outside.example>\r\n\
References: <dsp1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 10:00:00 +0900\r\n\
\r\n\
Confidential: the shortlist is Quillfeather.\r\n";

const TID: &str = "dsp1@outside.example";

/// `user`'s row for the conversation, `(subject, preview)`, per core.
async fn row(c: &Cores, d: &mut Diffs, user: &str) -> [(String, String); 2] {
    let inbox = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            ..Default::default()
        },
    };
    let (fc, pg) = d
        .both(
            &format!("list_conversations for {user}"),
            c.fastcore.list_conversations(user, &inbox).await,
            c.pg_core.list_conversations(user, &inbox).await,
            MINTED,
        )
        .unwrap_or_else(|| panic!("both cores list {user}'s mail"));
    let of = |items: &[mailrs_core_api::types::ConversationSummaryWire]| {
        items
            .iter()
            .find(|r| r.thread_id == TID)
            .map(|r| (r.subject.clone(), r.snippet.clone()))
            .unwrap_or_else(|| panic!("{user} holds the conversation"))
    };
    [of(&fc.items), of(&pg.items)]
}

/// How many conversations `user` finds with `q`, per core.
async fn found(c: &Cores, user: &str, q: &str) -> [usize; 2] {
    let req = SearchConversationsRequest {
        query: q.into(),
        category: None,
        limit: 20,
        folder: None,
        unread: None,
        starred: None,
        archived: false,
    };
    let fc = c.fastcore.search_conversations(user, &req).await;
    let pg = c.pg_core.search_conversations(user, &req).await;
    [
        fc.expect("search").items.len(),
        pg.expect("search").items.len(),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reader_sees_and_finds_only_their_own_messages() {
    let cores = harness::start_on(
        "mem://display-fastcore-side-state",
        "mem://display-pg-core-side-state",
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
    let to_alice = SpoolEnvelope {
        reverse_path: "dave@outside.example".into(),
        ..envelope(None)
    };
    spools.arrive(&cores, &to_alice, DAVES).await;

    for (core, (subject, preview)) in ["fastcore", "pg-core"]
        .into_iter()
        .zip(row(&cores, &mut d, BOB).await)
    {
        assert_eq!(subject, "Friday agenda", "{core}: Bob's subject");
        assert!(
            preview.starts_with("Here is the agenda"),
            "{core}: Bob's preview {preview:?}"
        );
    }
    for (core, (subject, preview)) in ["fastcore", "pg-core"]
        .into_iter()
        .zip(row(&cores, &mut d, ALICE).await)
    {
        assert_eq!(subject, "Re: Friday agenda", "{core}: Alice's subject");
        assert!(
            preview.contains("Quillfeather"),
            "{core}: Alice's preview {preview:?}"
        );
    }
    for q in ["Quillfeather", "Zimmerman", "shortlist"] {
        assert_eq!(found(&cores, BOB, q).await, [0, 0], "Bob finds it by {q:?}");
        assert_eq!(
            found(&cores, ALICE, q).await,
            [1, 1],
            "Alice finds it by {q:?}"
        );
    }
    assert_eq!(
        found(&cores, BOB, "agenda").await,
        [1, 1],
        "Bob finds his own"
    );

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
