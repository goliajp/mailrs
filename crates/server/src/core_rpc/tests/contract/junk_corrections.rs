//! A reader taking a conversation out of Junk, on both cores: whichever
//! verb files it and whoever put it there, the correction is recorded and
//! the conversation is learnt as ham.

use mailrs_reputation::{Source, Stance};

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::harness::{self, Diffs};
use super::intake::{Spools, envelope};
use super::{ALICE, accounts};

/// Ids and counters each store keeps for itself.
const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

const NEWSLETTER: &[u8] = b"From: Shop News <news@shop.example>\r\n\
To: alice@example.test\r\n\
Subject: This week's picks\r\n\
Message-ID: <n1@shop.example>\r\n\
List-Unsubscribe: <https://shop.example/u>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
Three new things for spring.\r\n";

/// The vouches and retractions the reader's correction left in a core's
/// side state.
fn corrections(url: &str) -> (Vec<(String, String)>, usize) {
    let mut c = kevy_client::Connection::connect(url).expect("kevy");
    let all = mailrs_core_sidestate::families::screening::observations(&mut c).expect("obs");
    let vouches = all
        .iter()
        .filter(|o| o.source == Source::UserNotJunk && o.stance == Stance::Vouch)
        .map(|o| (o.value.clone(), o.reason.clone()))
        .collect();
    let retracts = all.iter().filter(|o| o.stance == Stance::Retract).count();
    (vouches, retracts)
}

/// The direction the spam corpus the receiver reads last learnt `tid`.
fn trained(url: &str, tid: &str) -> Option<bool> {
    let mut c = kevy_client::Connection::connect(url).expect("kevy");
    mailrs_triage::spam::trained_as_spam(&mut c, mailrs_triage::spam::READ, tid)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn filing_out_of_junk_is_a_correction_on_both_cores() {
    let (fc_kevy, pg_kevy) = (
        "mem://junk-corrections-fastcore",
        "mem://junk-corrections-pg-core",
    );
    let cores = harness::start_on(fc_kevy, pg_kevy).await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);
    // the receiver put it in Junk
    let mut env = envelope(None);
    env.target_folder = "Junk".into();
    spools.arrive(&cores, &env, NEWSLETTER).await;

    let tid = "n1@shop.example";
    let junk = ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some("Junk".into()),
            limit: 50,
            ..Default::default()
        },
    };
    let (_, rows) = d
        .both(
            "list_conversations [Junk] after it arrived",
            cores.fastcore.list_conversations(ALICE, &junk).await,
            cores.pg_core.list_conversations(ALICE, &junk).await,
            MINTED,
        )
        .expect("both list");
    assert!(rows.items.iter().any(|r| r.thread_id == tid), "not in Junk");

    d.both(
        "mark_promotion out of Junk",
        cores.fastcore.mark_promotion(ALICE, tid).await,
        cores.pg_core.mark_promotion(ALICE, tid).await,
        MINTED,
    );

    let reason = format!("took conversation {tid} out of Junk");
    for url in [fc_kevy, pg_kevy] {
        let (vouches, retracts) = corrections(url);
        assert_eq!(
            vouches,
            [("shop.example".to_string(), reason.clone())],
            "{url}"
        );
        assert_eq!(retracts, 0, "{url}: nothing of the reader's to retract");
        assert_eq!(trained(url, tid), Some(false), "{url}: not learnt as ham");
    }

    // filed again from Promotions: not out of Junk, nothing more recorded
    d.both(
        "move_to_inbox from Promotions",
        cores.fastcore.move_to_inbox(ALICE, tid).await,
        cores.pg_core.move_to_inbox(ALICE, tid).await,
        MINTED,
    );
    for url in [fc_kevy, pg_kevy] {
        assert_eq!(corrections(url).0.len(), 1, "{url}");
    }
    assert!(d.0.is_empty(), "{:?}", d.0);
}
