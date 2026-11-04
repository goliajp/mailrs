//! Curated spam for the v2 spam/ham corpus: the conversations readers
//! moved into Junk themselves and left there. Against a real kevy-server
//! for the shared facts, an embedded store and a maildir on disk. Every
//! message is synthetic.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};
use mailrs_reputation::{Kind, Observation, Source, Stance};
use mailrs_triage::spam::{Corpus, keys};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const DAY: i64 = 86_400;

fn add(
    store: &KevyMailboxStore,
    root: &Path,
    now: i64,
    tid: &str,
    from: &str,
    category: &str,
    age_days: i64,
) {
    let mid = format!("{tid}@mail.example");
    let raw = format!(
        "From: Sender <{from}>\r\nTo: {USER}\r\nSubject: {tid}\r\nMessage-ID: <{mid}>\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\nclaim your prize now\r\n"
    );
    let md = mailrs_maildir::Maildir::open(root.join("x.com/bob"));
    let blob = md.deliver(raw.as_bytes()).unwrap().0;
    let date = now - age_days * DAY;
    let uid = store.allocate_uid(USER, &mid).unwrap();
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob, "sender": format!("Sender <{from}>"),
        "recipients": USER, "subject": tid, "date": date, "internal_date": date, "size": raw.len(),
        "flags": 0, "message_id": mid, "in_reply_to": "", "thread_id": tid, "modseq": 1
    });
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: tid.into(),
                subject: tid.into(),
                senders_csv: from.into(),
                count: 1,
                unread_count: 0,
                latest_date: date,
                latest_preview: String::new(),
                category: category.into(),
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
            &mid,
            date,
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

/// The fact the reader's "move to Junk" leaves: one UserJunk mark.
fn mark(c: &mut kevy_client::Connection, now: i64, tid: &str, domain: &str) -> String {
    mailrs_core_sidestate::families::screening::add_observation(
        c,
        Observation {
            id: String::new(),
            at: now - 20 * DAY,
            source: Source::UserJunk,
            kind: Kind::SenderDomain,
            value: domain.into(),
            stance: Stance::Deny,
            retracts: None,
            reason: format!("moved conversation {tid} to Junk"),
            actor: Some(USER.into()),
        },
    )
    .unwrap()
}

async fn rebuild(state: &Arc<FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:bayes-rebuild-v2?source=curated_spam&{query}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn curated_spam_takes_what_readers_moved_into_junk_and_left_there() {
    let (_k, url) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("maildir");
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/bob").join(leaf)).unwrap();
    }
    // one test in its own integration-test process owns this variable
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .unwrap();
    let now = chrono::Utc::now().timestamp();
    let mut c = kevy_client::Connection::connect(&url).unwrap();
    // kept: moved into Junk, still there, old enough
    add(&store, &root, now, "s1", "a@prize.example", "spam", 30);
    mark(&mut c, now, "s1", "prize.example");
    // the reader took it back out: the mark is retracted
    add(&store, &root, now, "s2", "a@back.example", "inbox", 30);
    let id = mark(&mut c, now, "s2", "back.example");
    mailrs_core_sidestate::families::screening::add_observation(
        &mut c,
        Observation {
            id: String::new(),
            at: now - 10 * DAY,
            source: Source::UserJunk,
            kind: Kind::SenderDomain,
            value: "back.example".into(),
            stance: Stance::Retract,
            retracts: Some(id),
            reason: "moved conversation s2 out of Junk".into(),
            actor: Some(USER.into()),
        },
    )
    .unwrap();
    // not in Junk any more, whatever the marks say
    add(&store, &root, now, "s3", "a@moved.example", "inbox", 30);
    mark(&mut c, now, "s3", "moved.example");
    // too recent to be sure of
    add(&store, &root, now, "s4", "a@prize.example", "spam", 3);
    mark(&mut c, now, "s4", "prize.example");
    // already learnt as spam from the legacy corpus
    add(&store, &root, now, "s5", "a@prize.example", "spam", 30);
    mark(&mut c, now, "s5", "prize.example");
    c.sadd(keys(Corpus::Legacy).corpus_spam, &[b"s5".as_slice()])
        .unwrap();
    // in Junk but put there by the classifier: no reader fact, no candidate
    add(&store, &root, now, "s6", "a@prize.example", "spam", 30);
    // one sender, three conversations, for the per-domain cap
    for tid in ["s7", "s8", "s9"] {
        add(&store, &root, now, tid, "b@heavy.example", "spam", 40);
        mark(&mut c, now, tid, "heavy.example");
    }
    // the reader's own message
    add(&store, &root, now, "s10", USER, "spam", 30);
    mark(&mut c, now, "s10", "x.com");

    let mut state = FastcoreState::new(store);
    state.net_url = Some(url.clone());
    state.authserv_id = Some("mail.golia.ai".into());
    let state = Arc::new(state);

    let dry = rebuild(&state, "").await;
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(
        dry["candidates"], 9,
        "every marked conversation but s6: {dry}"
    );
    assert_eq!(dry["kept"], 4, "s1, s7-9: {dry}");
    let r = &dry["rejected"];
    assert_eq!(r["own_only"], 1, "s10: {dry}");
    assert_eq!(r["released"], 1, "s2: {dry}");
    assert_eq!(r["not_in_junk_now"], 1, "s3: {dry}");
    assert_eq!(r["recent"], 1, "s4: {dry}");
    assert_eq!(r["already_in_corpus"], 1, "s5: {dry}");

    let capped = rebuild(&state, "per_domain_cap=2").await;
    assert_eq!(capped["kept"], 3, "{capped}");
    assert_eq!(capped["rejected"]["per_domain_cap"], 1, "{capped}");
    assert_eq!(capped, rebuild(&state, "per_domain_cap=2").await);

    let done = rebuild(&state, "dry_run=false").await;
    assert_eq!(done["walked"], 4, "{done}");
    let spam_msgs = c
        .hget(keys(Corpus::V2).meta, b"spam_msgs")
        .unwrap()
        .map(|v| String::from_utf8(v).unwrap());
    assert_eq!(spam_msgs.as_deref(), Some("4"));
    for kept in ["s1", "s7", "s8", "s9"] {
        assert_eq!(
            mailrs_triage::spam::trained_as_spam(&mut c, Corpus::V2, kept),
            Some(true),
            "{kept}"
        );
    }
    for left in ["s2", "s3", "s4", "s6", "s10"] {
        assert_eq!(
            mailrs_triage::spam::trained_as_spam(&mut c, Corpus::V2, left),
            None,
            "{left}"
        );
    }
    assert_eq!(c.hget(keys(Corpus::V2).meta, b"ham_msgs").unwrap(), None);
}
