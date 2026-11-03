//! A store with mail from one sender, observations about that sender, and
//! the routes a re-judging round and its batches answer on.

#![allow(dead_code)] // each test file uses a different part

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::ledger::LedgerFacts;
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};
use mailrs_reputation::{Kind, Observation, Source, Stance};
use tower::ServiceExt;

pub const USER: &str = "bob@x.com";
pub const FROM: &str = "billing@mail.vendorbill.com";

/// One letter from [`FROM`]: an account number to pay into (a first
/// contact, so `bec-pay-to-a-stranger`), and with `program` a `.exe`
/// attached (`executable-attachment`, which holds).
pub fn letter(mid: &str, program: bool) -> Vec<u8> {
    let attachment = match program {
        true => {
            "--b\r\nContent-Type: application/octet-stream\r\n\
Content-Disposition: attachment; filename=\"setup.exe\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nTVqQAAMAAAAEAAAA\r\n"
        }
        false => "",
    };
    format!(
        "From: Billing <{FROM}>\r\nTo: {USER}\r\nSubject: Invoice {mid}\r\n\
Message-ID: <{mid}>\r\nDate: Mon, 2 Nov 2026 09:00:00 +0000\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n\
Please pay the invoice to IBAN DE89 3704 0044 0532 0130 00 this week.\r\n\
{attachment}--b--\r\n"
    )
    .into_bytes()
}

pub struct World {
    pub state: Arc<FastcoreState>,
    pub kv: kevy_client::Connection,
    pub root: PathBuf,
    _tmp: tempfile::TempDir,
}

/// A fresh store whose changes can be counted, and `MAILRS_MAILDIR` set:
/// call once per test process.
pub fn world(kevy: &str) -> World {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("maildir");
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/bob").join(leaf)).unwrap();
    }
    // one test in its own integration-test process owns this variable
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default().with_feed(1 << 20)).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store.ensure_ledger_tables().unwrap();
    store
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .unwrap();
    let mut state = FastcoreState::new(store);
    state.net_url = Some(kevy.into());
    World {
        state: Arc::new(state),
        kv: kevy_client::Connection::connect(kevy).unwrap(),
        root,
        _tmp: tmp,
    }
}

/// Deliver `raw` as its own conversation `mid`, already read.
pub fn add(w: &World, mid: &str, raw: &[u8]) {
    let store = &w.state.mailbox;
    let md = mailrs_maildir::Maildir::open(w.root.join("x.com/bob"));
    let blob = md.deliver(raw).unwrap().0;
    let uid = store.allocate_uid(USER, mid).unwrap();
    let at = 1_788_000_000i64;
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob,
        "sender": format!("Billing <{FROM}>"), "recipients": USER,
        "subject": format!("Invoice {mid}"), "date": at,
        "internal_date": at, "size": raw.len(), "flags": 1,
        "message_id": mid, "in_reply_to": "", "thread_id": mid, "modseq": 1
    });
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: mid.into(),
                subject: format!("Invoice {mid}"),
                senders_csv: format!("Billing <{FROM}>"),
                count: 1,
                unread_count: 0,
                latest_date: at,
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
            mid,
            mid,
            at,
            &serde_json::to_vec(&wire).unwrap(),
            &UserMessageFacts {
                blob_ref: &blob,
                uid,
                flags: 1,
                modseq: 1,
            },
        )
        .unwrap();
    let facts = LedgerFacts {
        message_id: mid.into(),
        user: USER.into(),
        from_addr: FROM.into(),
        from_domain: "mail.vendorbill.com".into(),
        from_reg: "vendorbill.com".into(),
        arrived: chrono::Utc::now().timestamp(),
        ..LedgerFacts::default()
    };
    let until = chrono::Utc::now().timestamp() + 86_400;
    store
        .record_ledger_indexed(&facts, &[], Some(until))
        .unwrap();
}

pub fn observe(conn: &mut kevy_client::Connection, stance: Stance, value: &str) -> String {
    observe_retracting(conn, stance, value, None)
}

pub fn observe_retracting(
    conn: &mut kevy_client::Connection,
    stance: Stance,
    value: &str,
    retracts: Option<String>,
) -> String {
    mailrs_core_sidestate::families::screening::add_observation(
        conn,
        Observation {
            id: String::new(),
            at: chrono::Utc::now().timestamp(),
            source: Source::Manual,
            kind: Kind::SenderDomain,
            value: value.into(),
            stance,
            retracts,
            reason: "test".into(),
            actor: Some("admin@x.com".into()),
        },
    )
    .unwrap()
}

pub async fn call(
    state: &Arc<FastcoreState>,
    method: Method,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

/// One round; how many messages it judged, and the counts since start.
pub async fn round(state: &Arc<FastcoreState>) -> (u64, serde_json::Value) {
    let (status, v) = call(
        state,
        Method::POST,
        "/v1/admin/maintenance:rejudge-now",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    (v["judged"].as_u64().unwrap(), v["since_start"].clone())
}

/// Where the store's change feed stands: equal before and after means
/// nothing was written in between.
pub fn tail(state: &FastcoreState) -> (u64, u64) {
    state.mailbox.store_ref().changes_tail().unwrap()
}

pub fn held(state: &FastcoreState, mid: &str) -> bool {
    state
        .mailbox
        .get_thread_for_user(USER, mid)
        .unwrap()
        .unwrap()
        .quarantined
}
