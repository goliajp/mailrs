//! A rule that only scores still reaches mail delivered before it
//! existed: the backfill moves a conversation to Junk when its fraud
//! score alone reaches the threshold, and leaves everything else where
//! it was. Its own process, because it owns `MAILRS_*` variables.
use std::{path::Path, sync::Arc};

use mailrs_fastcore::{FastcoreState, fraud_backfill::run_once};
use mailrs_mailbox_kevy::keys::{Bucket, bucket_of};
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};

const USER: &str = "lihao@x.com";

fn add(store: &KevyMailboxStore, root: &Path, tid: &str, subject: &str) {
    let raw = format!(
        "From: Erica Flores <ericaf_flores@caredealspark.com>\r\nTo: lihao@x.com\r\n\
         Subject: {subject}\r\n\r\n{subject}, would 4 to 8 more contracts move the needle?\r\n"
    );
    let md = mailrs_maildir::Maildir::open(root.join("x.com/lihao"));
    let blob = md.deliver(raw.as_bytes()).unwrap().0;
    let uid = store.allocate_uid(USER, tid).unwrap();
    let wire = serde_json::json!({
        "id":0,"mailbox_id":0,"uid":uid,"blob_ref":blob,
        "sender":"Erica Flores <ericaf_flores@caredealspark.com>","recipients":USER,
        "subject":subject,"date":1790000000i64,"internal_date":1790000000i64,
        "size":raw.len(),"flags":0,"message_id":tid,"in_reply_to":"",
        "thread_id":tid,"modseq":1
    });
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: tid.into(),
                subject: subject.into(),
                senders_csv: "Erica Flores <ericaf_flores@caredealspark.com>".into(),
                count: 1,
                unread_count: 1,
                latest_date: 1790000000,
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
                starred: false,
            },
        )
        .unwrap();
    store
        .upsert_user_message(
            USER,
            tid,
            tid,
            1790000000,
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

fn bucket(state: &FastcoreState, tid: &str) -> Bucket {
    let row = state
        .mailbox
        .get_thread_for_user(USER, tid)
        .unwrap()
        .unwrap();
    assert!(
        !row.quarantined,
        "{tid} must not be held — a score is not a verdict"
    );
    bucket_of(&row.category)
}

#[tokio::test]
async fn the_backfill_junks_a_scored_campaign_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("maildir");
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/lihao").join(leaf)).unwrap();
    }
    let journal = tmp.path().join("job");
    let rules = tmp.path().join("active.lua");
    std::fs::write(&rules, mailrs_fraud_lua::DEFAULT_SOURCE).unwrap();
    // One test in its own integration-test process owns these variables.
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", &root);
        std::env::set_var("MAILRS_FRAUD_RULES_FILE", &rules);
        std::env::set_var("MAILRS_KEVY_URL", "mem://fraud-backfill-junks-scored");
    }
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store
        .upsert_account(
            USER,
            r#"{"address":"lihao@x.com","active":true,"display_name":"LI HAO"}"#,
        )
        .unwrap();
    add(&store, &root, "cut", "Hao");
    add(&store, &root, "plain", "Quarterly contracts");
    let state = Arc::new(FastcoreState::new(store));

    assert!(!run_once(state.clone(), &journal, &rules).await.unwrap());
    let progress: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal.join("progress.json")).unwrap()).unwrap();
    assert_eq!(progress["complete"], true);
    assert_eq!(progress["junked"], 1, "{progress}");
    assert_eq!(progress["held"], 0, "{progress}");

    assert_eq!(bucket(&state, "cut"), Bucket::Junk);
    assert_ne!(
        bucket(&state, "plain"),
        Bucket::Junk,
        "the control must not move"
    );

    // Once the recipient says the sender is not junk, a later sweep
    // leaves their mail where it is — the receive path already does.
    let mut kevy = kevy_client::Connection::connect("mem://fraud-backfill-junks-scored").unwrap();
    kevy.sadd(
        mailrs_core_sidestate::families::sender_lists::whitelist_key(USER).as_bytes(),
        &[b"ericaf_flores@caredealspark.com".as_slice()],
    )
    .unwrap();
    add(&state.mailbox, &root, "cut-again", "Hao");
    let journal = tmp.path().join("job-after-not-junk");
    assert!(!run_once(state.clone(), &journal, &rules).await.unwrap());
    let progress: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal.join("progress.json")).unwrap()).unwrap();
    assert_eq!(progress["junked"], 0, "{progress}");
    assert_ne!(bucket(&state, "cut-again"), Bucket::Junk);
}
