//! A reputation observation sends the mail it names back through the rules:
//! a link host, a sender domain with its subdomains, and nothing for an
//! address or a never-listed host. With nothing new, a round writes nothing.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::ledger::{IndexedLink, LedgerFacts};
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};
use mailrs_reputation::{Kind, Observation, Source, Stance};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const KEVY: &str = "mem://fastcore-rejudge-follows";
const MID: &str = "invoice-1@mail.vendor.example";
const RAW: &[u8] = b"From: Support <support@mail.vendor.example>\r\n\
To: bob@x.com\r\n\
Subject: Your invoice\r\n\
Message-ID: <invoice-1@mail.vendor.example>\r\n\
\r\n\
Pay at https://pay-here.example/now or read https://docs.google.com/x\r\n";

fn add(store: &KevyMailboxStore, root: &Path) {
    let md = mailrs_maildir::Maildir::open(root.join("x.com/bob"));
    let blob = md.deliver(RAW).unwrap().0;
    let uid = store.allocate_uid(USER, MID).unwrap();
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob,
        "sender": "Support <support@mail.vendor.example>", "recipients": USER,
        "subject": "Your invoice", "date": 1_788_000_000i64,
        "internal_date": 1_788_000_000i64, "size": RAW.len(), "flags": 0,
        "message_id": MID, "in_reply_to": "", "thread_id": MID, "modseq": 1
    });
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: MID.into(),
                subject: "Your invoice".into(),
                senders_csv: "Support <support@mail.vendor.example>".into(),
                count: 1,
                unread_count: 1,
                latest_date: 1_788_000_000,
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
            MID,
            MID,
            1_788_000_000,
            &serde_json::to_vec(&wire).unwrap(),
            &UserMessageFacts {
                blob_ref: &blob,
                uid,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
    let facts = LedgerFacts {
        message_id: MID.into(),
        user: USER.into(),
        from_addr: "support@mail.vendor.example".into(),
        from_domain: "mail.vendor.example".into(),
        from_reg: "vendor.example".into(),
        link_hosts: vec!["pay-here.example".into(), "docs.google.com".into()],
        arrived: chrono::Utc::now().timestamp(),
        ..LedgerFacts::default()
    };
    let pay = IndexedLink {
        host: "pay-here.example".into(),
        reg: "pay-here.example".into(),
    };
    let until = chrono::Utc::now().timestamp() + 86_400;
    store
        .record_ledger_indexed(&facts, &[pay], Some(until))
        .unwrap();
}

fn observe(conn: &mut kevy_client::Connection, kind: Kind, value: &str) {
    mailrs_core_sidestate::families::screening::add_observation(
        conn,
        Observation {
            id: String::new(),
            at: chrono::Utc::now().timestamp(),
            source: Source::Manual,
            kind,
            value: value.into(),
            stance: Stance::Deny,
            retracts: None,
            reason: "test".into(),
            actor: Some("admin@x.com".into()),
        },
    )
    .unwrap();
}

async fn round(state: &Arc<FastcoreState>) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:rejudge-now")
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
async fn an_observation_rejudges_the_mail_it_names_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("maildir");
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/bob").join(leaf)).unwrap();
    }
    // one test in its own integration-test process owns this variable
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };
    let mut kv = kevy_client::Connection::connect(KEVY).unwrap();
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store.ensure_ledger_tables().unwrap();
    store
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .unwrap();
    add(&store, &root);
    let mut state = FastcoreState::new(store);
    state.net_url = Some(KEVY.into());
    let state = Arc::new(state);

    // the first round starts from the counter as it stands
    assert_eq!(round(&state).await["judged"], 0);
    // nothing new: nothing judged, and no verdict written
    assert_eq!(round(&state).await["judged"], 0);
    assert!(state.mailbox.ledger_verdict(USER, MID).unwrap().is_none());

    observe(&mut kv, Kind::LinkDomain, "pay-here.example");
    assert_eq!(round(&state).await["judged"], 1, "the link host's mail");
    let v = state.mailbox.ledger_verdict(USER, MID).unwrap().unwrap();
    assert!(
        !v.rules_version.is_empty(),
        "judged with the rules in force"
    );

    observe(&mut kv, Kind::SenderDomain, "vendor.example");
    assert_eq!(
        round(&state).await["judged"],
        1,
        "a parent domain finds its subdomain's mail"
    );

    observe(&mut kv, Kind::LinkDomain, "docs.google.com");
    assert_eq!(
        round(&state).await["judged"],
        0,
        "a never-listed host moves nothing"
    );
    observe(&mut kv, Kind::Ip, "203.0.113.9");
    assert_eq!(round(&state).await["judged"], 0, "an address has no index");

    // shadow: nothing was held, whatever the verdict
    let row = state
        .mailbox
        .get_thread_for_user(USER, MID)
        .unwrap()
        .unwrap();
    assert!(!row.quarantined);
}
