//! Exercise the real apply path, including a local copy of the reported mail.
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

#[tokio::test]
async fn snapshot_resume_reload_and_failure_preserve_progress_and_mail() {
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
    let raw = std::env::var_os("MAILRS_FRAUD_COPY_MESSAGE")
        .map(|path| std::fs::read(path).unwrap())
        .unwrap_or_else(|| b"From: ChatGPT <admin@heavenerandassociates.com>\r\nSubject: payment failed\r\n\r\nUpdate payment method\r\n".to_vec());
    for i in 0..105 {
        add(&store, &root, &format!("t{i:03}"), &raw);
    }
    let state = Arc::new(FastcoreState::new(store));
    assert!(run_once(state.clone(), &journal, &rules).await.unwrap());
    assert_eq!(progress(&journal)["cursor"], 100);
    assert_eq!(progress(&journal)["held"], 100);
    let row = state
        .mailbox
        .get_thread_for_user(USER, "t000")
        .unwrap()
        .unwrap();
    assert!(row.quarantined);
    assert_eq!(row.unread_count, 0);
    assert_eq!(
        state
            .mailbox
            .user_message_facts(USER, "t000")
            .unwrap()
            .unwrap()
            .flags
            & 1,
        1
    );
    assert_eq!(
        std::fs::read_dir(root.join("x.com/bob/cur"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(":2,S"))
            .count(),
        100
    );

    // A new key sorting before the cursor cannot move the durable snapshot.
    add(&state.mailbox, &root, "a-new-arrival", &raw);
    // Discarding all worker state simulates process restart: only the journal
    // carries the cursor into the second call.
    assert!(!run_once(state.clone(), &journal, &rules).await.unwrap());
    assert_eq!(progress(&journal)["held"], 105);
    assert!(
        state
            .mailbox
            .get_thread_for_user(USER, "t104")
            .unwrap()
            .unwrap()
            .quarantined
    );
    assert!(
        !state
            .mailbox
            .get_thread_for_user(USER, "a-new-arrival")
            .unwrap()
            .unwrap()
            .quarantined
    );
    let saved = std::fs::read(journal.join("progress.json")).unwrap();
    let modified = std::fs::metadata(journal.join("progress.json"))
        .unwrap()
        .modified()
        .unwrap();
    assert!(!run_once(state.clone(), &journal, &rules).await.unwrap());
    assert_eq!(
        std::fs::metadata(journal.join("progress.json"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );

    std::fs::write(&rules, "this is invalid Lua").unwrap();
    assert!(run_once(state.clone(), &journal, &rules).await.is_err());
    assert_eq!(std::fs::read(journal.join("progress.json")).unwrap(), saved);

    // Registration succeeds, but real messages fail. Fallback may still protect
    // receipt; it must not count as completing a backfill under the new version.
    std::fs::write(
        &rules,
        "rule('broken','identity',6,true,function(m) if m.from ~= '' then error('bad') end end)",
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    assert!(run_once(state.clone(), &journal, &rules).await.is_err());
    assert_eq!(progress(&journal)["cursor"], 0);
    assert!(
        state
            .mailbox
            .get_thread_for_user(USER, "t000")
            .unwrap()
            .unwrap()
            .quarantined
    );

    // A subsequent rule version starts a new snapshot and can release old holds.
    std::fs::write(
        &rules,
        "rule('clear','identity',1,false,function() return nil end)",
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    assert!(run_once(state.clone(), &journal, &rules).await.unwrap());
    assert!(!run_once(state.clone(), &journal, &rules).await.unwrap());
    assert_eq!(progress(&journal)["total"], 106);
    assert_eq!(progress(&journal)["released"], 105);
    assert!(progress(&journal)["complete"].as_bool().unwrap());
    assert!(
        !state
            .mailbox
            .get_thread_for_user(USER, "t104")
            .unwrap()
            .unwrap()
            .quarantined
    );
    assert_eq!(
        state
            .mailbox
            .get_thread_for_user(USER, "t104")
            .unwrap()
            .unwrap()
            .unread_count,
        0
    );
}
