//! Mail a user submitted over SMTP AUTH, drained by each core: the
//! sender's copy is in their Sent, counted as sent, alike on both cores
//! and like the copy a web send files; the same submission again files no
//! second copy.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};

use super::harness::{self, Cores, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, BOB, accounts};

const MINTED: &[&str] = &[
    "id",
    "mailbox_id",
    "uid",
    "created_at",
    "modseq",
    "blob_ref",
];

const SUBMITTED: &[u8] = b"Received: from app.example.org by mx.example.test\r\n\
From: Alice <alice@example.test>\r\n\
To: bob@example.test\r\n\
Subject: Lunch\r\n\
Message-ID: <smtp1@example.test>\r\n\
Date: Wed, 5 Nov 2026 10:00:00 +0900\r\n\
\r\n\
Noon?\r\n";

const WEB: &[u8] = b"From: Alice <alice@example.test>\r\n\
To: bob@example.test\r\n\
Subject: Dinner\r\n\
Message-ID: <web1@example.test>\r\n\
Date: Wed, 5 Nov 2026 10:05:00 +0900\r\n\
\r\n\
Seven?\r\n";

async fn sent_rows(c: &Cores, d: &mut Diffs, when: &str) -> Vec<ConversationSummaryWire> {
    let req = ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some("Sent".into()),
            limit: 50,
            ..Default::default()
        },
    };
    d.both(
        &format!("list_conversations [Sent] {when}"),
        c.fastcore.list_conversations(ALICE, &req).await,
        c.pg_core.list_conversations(ALICE, &req).await,
        MINTED,
    )
    .map(|(_, pg)| pg.items)
    .unwrap_or_default()
}

fn sender_side(rows: &[ConversationSummaryWire], tid: &str) -> serde_json::Value {
    let r = rows
        .iter()
        .find(|r| r.thread_id == tid)
        .unwrap_or_else(|| panic!("{tid} is not in Sent: {rows:?}"));
    serde_json::json!({
        "message_count": r.message_count,
        "unread_count": r.unread_count,
        "sent_count": r.sent_count,
        "category": r.category,
        "archived": r.archived,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_submission_files_the_senders_copy_on_both_cores() {
    let cores = harness::start_on(
        "mem://submission-fastcore-side-state",
        "mem://submission-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    let mut env = envelope(None);
    env.reverse_path = ALICE.into();
    env.forward_paths = vec![BOB.into()];
    env.is_authenticated = true;
    env.auth_user = Some(ALICE.into());
    spools.arrive(&cores, &env, SUBMITTED).await;
    d.both(
        "file_sent the web send",
        cores
            .fastcore
            .file_sent(ALICE, WEB, &[BOB.to_string()])
            .await,
        cores
            .pg_core
            .file_sent(ALICE, WEB, &[BOB.to_string()])
            .await,
        &["blob_ref"],
    );

    let rows = sent_rows(&cores, &mut d, "after a submission").await;
    let submitted = sender_side(&rows, "smtp1@example.test");
    assert_eq!(submitted["sent_count"], 1, "{submitted}");
    assert_eq!(submitted, sender_side(&rows, "web1@example.test"));
    d.both(
        "list_sent_messages after a submission",
        cores.fastcore.list_sent_messages(ALICE).await,
        cores.pg_core.list_sent_messages(ALICE).await,
        MINTED,
    );

    spools.arrive(&cores, &env, SUBMITTED).await;
    let rows = sent_rows(&cores, &mut d, "after the same submission again").await;
    assert_eq!(sender_side(&rows, "smtp1@example.test"), submitted);

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
