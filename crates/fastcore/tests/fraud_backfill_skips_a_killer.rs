//! A conversation the backfill dies on at every start is skipped and recorded.
use std::{path::Path, sync::Arc};

use mailrs_fastcore::{FastcoreState, fraud_backfill::run_once};
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};

const USER: &str = "bob@x.com";

fn add(store: &KevyMailboxStore, root: &Path, tid: &str, raw: &[u8]) {
    let md = mailrs_maildir::Maildir::open(root.join("x.com/bob"));
    let blob = md.deliver(raw).unwrap().0;
    let uid = store.allocate_uid(USER, tid).unwrap();
    let wire = serde_json::json!({
        "id":0,"mailbox_id":0,"uid":uid,"blob_ref":blob,
        "sender":"ChatGPT <admin@heavenerandassociates.com>","recipients":USER,
        "subject":"payment failed","date":1788657272i64,"internal_date":1788657272i64,
        "size":raw.len(),"flags":0,"message_id":tid,"in_reply_to":"",
        "thread_id":tid,"modseq":1
    });
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: tid.into(),
                subject: "payment failed".into(),
                senders_csv: "ChatGPT <admin@heavenerandassociates.com>".into(),
                count: 1,
                unread_count: 1,
                latest_date: 1788657272,
                latest_preview: String::new(),
                category: "inbox".into(),
                importance_level: "normal".into(),
                importance_score: 0.0,
                requires_action: false,
                pinned: false,
                archived: false,
                quarantined: false,
                has_action: false,
                sent_count: 0,
                snoozed_until: 0,
                trashed_at: 0,
                starred: false,
            },
        )
        .unwrap();
    store
        .upsert_user_message(
            USER,
            tid,
            tid,
            1788657272,
            &serde_json::to_vec(&wire).unwrap(),
            &UserMessageFacts {
                blob_ref: &blob,
                uid,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
}

fn progress(journal: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(journal.join("progress.json")).unwrap()).unwrap()
}

/// What a run that died judging leaves behind: the range it had started,
/// counted.
fn died_during(journal: &Path, cursor: usize, end: usize, attempts: u32, until: Option<usize>) {
    let version = progress(journal)["version"].clone();
    let flight = serde_json::json!({
        "version": version, "cursor": cursor, "end": end,
        "attempts": attempts, "one_by_one_until": until,
    });
    std::fs::write(
        journal.join("inflight.json"),
        serde_json::to_vec(&flight).unwrap(),
    )
    .unwrap();
}

fn held(state: &FastcoreState, tid: &str) -> bool {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .unwrap()
        .unwrap()
        .quarantined
}

/// A batch the process died in three times is judged one conversation at
/// a time; the conversation it then dies on is skipped and recorded, and
/// the rest of the backfill goes on.
#[tokio::test]
async fn a_conversation_that_kills_the_backfill_is_skipped_not_retried_forever() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("maildir");
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/bob").join(leaf)).unwrap();
    }
    let journal = tmp.path().join("job");
    let rules = tmp.path().join("active.lua");
    std::fs::write(&rules, mailrs_fraud_lua::DEFAULT_SOURCE).unwrap();
    // One test in its own integration-test process owns these variables.
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", &root);
        std::env::set_var("MAILRS_FRAUD_RULES_FILE", &rules);
    }
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store
        .upsert_account(
            USER,
            r#"{"address":"bob@x.com","active":true,"display_name":"Bob"}"#,
        )
        .unwrap();
    let raw = b"From: ChatGPT <admin@heavenerandassociates.com>\r\nSubject: payment failed\r\n\r\nUpdate payment method\r\n";
    for i in 0..105 {
        add(&store, &root, &format!("t{i:03}"), raw);
    }
    let state = Arc::new(FastcoreState::new(store));
    assert!(run_once(state.clone(), &journal, &rules).await.unwrap());
    assert_eq!(progress(&journal)["cursor"], 100);
    assert!(
        !journal.join("inflight.json").exists(),
        "a batch that finished left a record"
    );

    // the last batch, 100..105, stopped the process three times
    died_during(&journal, 100, 105, 3, None);
    assert!(run_once(state.clone(), &journal, &rules).await.unwrap());
    assert_eq!(
        progress(&journal)["cursor"],
        101,
        "not judged one at a time"
    );
    assert!(held(&state, "t100"));

    // and then t101, judged alone, stopped it too
    died_during(&journal, 101, 102, 1, Some(105));
    assert!(run_once(state.clone(), &journal, &rules).await.unwrap());
    assert_eq!(progress(&journal)["skipped"], 1);
    assert_eq!(
        progress(&journal)["cursor"],
        103,
        "t101 skipped, t102 judged"
    );
    assert!(!held(&state, "t101"), "the skipped conversation was judged");
    let skipped: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal.join("skipped.json")).unwrap()).unwrap();
    assert_eq!(skipped[0]["thread_id"], "t101");

    // the rest goes on, one at a time to the end of the batch that died
    assert!(run_once(state.clone(), &journal, &rules).await.unwrap());
    assert!(!run_once(state.clone(), &journal, &rules).await.unwrap());
    let p = progress(&journal);
    assert_eq!(p["cursor"], 105);
    assert!(p["complete"].as_bool().unwrap());
    assert_eq!(p["held"], 104);
    assert!(held(&state, "t104"));
    assert!(!journal.join("inflight.json").exists());
}
