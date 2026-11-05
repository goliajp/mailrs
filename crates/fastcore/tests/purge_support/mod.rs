//! Two accounts with mail on disk, a conversation they share and one only
//! bob has, side state for both in a real kevy-server: what the account
//! purge tests delete one of.

#![allow(dead_code)] // each test binary uses its own part

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kevy_client::Connection;
use tower::ServiceExt;

pub const BOB: &str = "bob@x.com";
pub const ALICE: &str = "alice@x.com";
pub const PASSWORD: &str = "correct horse battery";

fn message(mid: &str, subject: &str) -> String {
    format!(
        "From: carol@y.com\r\nTo: bob@x.com, alice@x.com\r\nSubject: {subject}\r\n\
         Message-ID: <{mid}>\r\nDate: Fri, 14 Aug 2026 01:00:00 +0000\r\n\r\nbody\r\n"
    )
}

pub fn deliver(root: &Path, user: &str, folder: &str, raw: &str) {
    let (local, domain) = user.split_once('@').unwrap();
    let dir = root.join(domain).join(local).join(folder);
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(dir.join(leaf)).unwrap();
    }
    mailrs_maildir::Maildir::open(&dir)
        .deliver(raw.as_bytes())
        .unwrap();
}

/// A key with its type and content, so two snapshots compare.
pub type Snapshot = BTreeMap<Vec<u8>, String>;

pub fn snapshot(c: &mut Connection, pattern: &str) -> Snapshot {
    let mut out = Snapshot::new();
    for key in c.keys(pattern.as_bytes()).unwrap() {
        let kind = c.type_of(&key).unwrap();
        let mut items: Vec<Vec<u8>> = match kind.as_str() {
            "string" => vec![c.get(&key).unwrap().unwrap_or_default()],
            "hash" => c
                .hgetall(&key)
                .unwrap()
                .chunks(2)
                .map(|p| p.concat())
                .collect(),
            "set" => c.smembers(&key).unwrap(),
            "zset" => c.zrange(&key, 0, -1).unwrap(),
            "list" => c.lrange(&key, 0, -1).unwrap(),
            other => vec![other.as_bytes().to_vec()],
        };
        items.sort();
        out.insert(key, format!("{kind}:{items:?}"));
    }
    out
}

pub fn names(s: &Snapshot) -> Vec<String> {
    s.keys()
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .collect()
}

pub fn mail_conn(state: &mailrs_fastcore::FastcoreState) -> Connection {
    Connection::Embedded(Box::new(state.mailbox.store_ref().clone()))
}

pub struct Fixture {
    pub _tmp: tempfile::TempDir,
    pub _kevy: Box<dyn std::any::Any>,
    pub root: PathBuf,
    pub kevy_url: String,
    pub state: Arc<mailrs_fastcore::FastcoreState>,
    pub shared_tid: String,
}

/// The fixture. Sets `MAILRS_MAILDIR` and `MAILRS_KEVY_URL`: one test per
/// binary.
pub async fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let (container, kevy_url) = mailrs_test_docker::kevy::kevy_server().await;
    // SAFETY: the only test in its binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", &root);
        std::env::set_var("MAILRS_KEVY_URL", &kevy_url);
    }
    deliver(&root, BOB, "", &message("shared@y.com", "team news"));
    deliver(&root, BOB, "", &message("solo@y.com", "for bob"));
    deliver(&root, BOB, ".Sent", &message("sent@y.com", "bob wrote"));
    deliver(&root, ALICE, "", &message("shared@y.com", "team news"));

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox.ensure_ledger_tables().unwrap();
    let hash = mailrs_core::users::UserStore::hash_password(PASSWORD).unwrap();
    // as the create route writes it: `created_at` in the blob, which the
    // store's index field is derived from
    let blob = serde_json::json!({
        "password_hash": hash,
        "active": true,
        "created_at": 1_786_000_000,
        "display_name": "Someone",
        "quota_bytes": 10_737_418_240_i64,
    })
    .to_string();
    for who in [BOB, ALICE] {
        mailbox.upsert_account(who, &blob).unwrap();
        mailbox.upsert_permissions(who, "{}").unwrap();
    }
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    mailrs_fastcore::self_heal_once(&state, BOB).await;
    let shared_tid = state
        .mailbox
        .thread_for_message_id(BOB, "shared@y.com")
        .unwrap()
        .expect("bob's shared thread");
    // alice's copy arrives the way delivery files it: the self-heal only
    // builds a conversation nobody has yet
    let alice_file = std::fs::read_dir(root.join("x.com/alice/new"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    let row = state
        .mailbox
        .user_message_view(BOB, "shared@y.com")
        .unwrap()
        .expect("bob's row");
    state
        .mailbox
        .deliver_message(
            &mailrs_mailbox_kevy::MessageArrival {
                thread_id: &shared_tid,
                user: ALICE,
                subject: "team news",
                senders_csv: "carol@y.com",
                latest_date: 1_786_669_200,
                latest_preview: "body",
                category: "inbox",
                unread: true,
                is_own: false,
            },
            "shared@y.com",
            &row,
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &alice_file,
                uid: 1,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
    state
        .mailbox
        .register_uid(ALICE, 1, "shared@y.com")
        .unwrap();
    state
        .mailbox
        .set_thread_for_message_id(ALICE, "shared@y.com", &shared_tid)
        .unwrap();

    let mut net = Connection::connect(&kevy_url).unwrap();
    for who in [BOB, ALICE] {
        net.set(mailrs_core_sidestate::sieve_key(who).as_bytes(), b"keep;")
            .unwrap();
        net.set(
            mailrs_core_sidestate::keys::quota::used_key(who).as_bytes(),
            b"300",
        )
        .unwrap();
        net.hset(
            mailrs_core_sidestate::families::prefs::drafts_key(who).as_bytes(),
            &[(b"1".as_slice(), b"{}".as_slice())],
        )
        .unwrap();
        mailrs_core_sidestate::families::permission_groups::add_member(&mut net, 1, who).unwrap();
    }
    Fixture {
        _tmp: tmp,
        _kevy: Box::new(container),
        root,
        kevy_url,
        state,
        shared_tid,
    }
}

pub async fn call(
    state: &Arc<mailrs_fastcore::FastcoreState>,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    let body = match body {
        Some(v) => axum::body::Body::from(v.to_string()),
        None => axum::body::Body::empty(),
    };
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// Bob's parked maildirs.
pub fn parked(root: &Path) -> Vec<String> {
    std::fs::read_dir(root.join(".deleted").join("x.com"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("bob."))
        .collect()
}
