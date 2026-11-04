//! Rebuilding the v2 corpora from what the old one recorded. Against a real
//! kevy-server: training is pipelined, which the in-process `mem://` client
//! does not do. Every message is synthetic.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};
use mailrs_triage::Corpus;
use mailrs_triage::spam::keys;
use tower::ServiceExt;

const USER: &str = "bob@x.com";

fn raw(subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: Sender <s@sender.example>\r\nTo: bob@x.com\r\nSubject: {subject}\r\n\
         Message-ID: <{subject}@sender.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\
         \r\n{body}\r\n"
    )
    .into_bytes()
}

/// A thread of one message owned by bob. With `file` its raw mail is in the
/// maildir; without, the rows name a file that is not there.
fn add(store: &KevyMailboxStore, root: &Path, tid: &str, category: &str, file: bool) {
    let mid = format!("{tid}@sender.example");
    let blob = match file {
        true => {
            let md = mailrs_maildir::Maildir::open(root.join("x.com/bob"));
            md.deliver(&raw(tid, "quarterly figures attached below"))
                .unwrap()
                .0
        }
        false => format!("1788000000.M1P1Q1.{tid}"),
    };
    let uid = store.allocate_uid(USER, &mid).unwrap();
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob,
        "sender": "s@sender.example", "recipients": USER, "subject": tid,
        "date": 1_788_000_000i64, "internal_date": 1_788_000_000i64, "size": 10,
        "flags": 0, "message_id": mid, "in_reply_to": "", "thread_id": tid, "modseq": 1
    });
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: tid.into(),
                subject: tid.into(),
                senders_csv: "s@sender.example".into(),
                count: 1,
                unread_count: 1,
                latest_date: 1_788_000_000,
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
}

/// bob's mailbox, and the old corpus sets in the network kevy at `url`:
///
/// - `t-spam`, `t-ham`: in one set each, raw mail on disk
/// - `t-both`: in both sets, the old marker says ham, raw mail on disk
/// - `t-both2`: in both sets, no marker
/// - `t-gone`: in the spam set, no account holds it
/// - `t-nofile`: in the ham set, held by bob, file missing
fn seeded(url: &str, root: &Path) -> Arc<FastcoreState> {
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/bob").join(leaf)).unwrap();
    }
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .unwrap();
    add(&store, root, "t-spam", "spam", true);
    add(&store, root, "t-ham", "inbox", true);
    add(&store, root, "t-both", "notification", true);
    add(&store, root, "t-both2", "promotion", true);
    add(&store, root, "t-nofile", "inbox", false);

    let mut c = kevy_client::Connection::connect(url).unwrap();
    let s: [&[u8]; 4] = [b"t-spam", b"t-both", b"t-both2", b"t-gone"];
    let legacy = keys(Corpus::Legacy);
    c.sadd(legacy.corpus_spam, &s).unwrap();
    let h: [&[u8]; 4] = [b"t-ham", b"t-both", b"t-both2", b"t-nofile"];
    c.sadd(legacy.corpus_ham, &h).unwrap();
    c.set(&legacy_marker("t-both"), b"ham").unwrap();

    let mut state = FastcoreState::new(store);
    state.net_url = Some(url.into());
    state.maildir_root = root.to_string_lossy().into_owned();
    Arc::new(state)
}

/// The raw-byte corpus's marker for `tid`, built from the corpus key
/// beside it: the family is spelled only in `mailrs_triage::spam`.
fn legacy_marker(tid: &str) -> Vec<u8> {
    let mut k = keys(Corpus::Legacy).corpus_spam.to_vec();
    k.truncate(k.len() - b"corpus:spam".len());
    k.extend_from_slice(b"trained:");
    k.extend_from_slice(tid.as_bytes());
    k
}

async fn rebuild(state: &Arc<FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/admin/maintenance:bayes-rebuild-v2?{query}"))
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

type Hash = Vec<(Vec<u8>, Vec<u8>)>;

/// The spam/ham counts of `corpus`, sorted, so two corpora compare as
/// values.
fn snapshot_of(url: &str, corpus: Corpus) -> Vec<(String, Hash)> {
    let mut c = kevy_client::Connection::connect(url).unwrap();
    let k = keys(corpus);
    [k.spam_tokens, k.ham_tokens, k.meta]
        .iter()
        .map(|k| {
            let flat = c.hgetall(k).unwrap();
            let mut all: Hash = flat
                .chunks(2)
                .map(|p| (p[0].clone(), p[1].clone()))
                .collect();
            all.sort();
            (String::from_utf8_lossy(k).into_owned(), all)
        })
        .collect()
}

#[tokio::test]
async fn the_spam_corpus_is_rebuilt_from_the_legacy_sets() {
    let (_a, url_a) = mailrs_test_docker::kevy::kevy_server().await;
    let (_b, url_b) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let one = seeded(&url_a, &tmp.path().join("a"));
    let paged = seeded(&url_b, &tmp.path().join("b"));

    // dry by default: it reports, and writes nothing
    let dry = rebuild(&one, "corpus=spam").await;
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(dry["roster"], 6, "{dry}");
    assert_eq!(dry["spam"], 1, "{dry}");
    assert_eq!(dry["ham"], 2, "t-ham and t-both, by its old marker: {dry}");
    assert_eq!(dry["in_both_sets"], 2, "{dry}");
    assert_eq!(dry["in_both_unknown"], 1, "{dry}");
    assert_eq!(dry["no_owner"], 1, "{dry}");
    assert_eq!(dry["no_raw"], 1, "{dry}");
    assert_eq!(dry["unresolved"], 3, "{dry}");
    assert_eq!(dry["done"], true, "{dry}");
    assert!(
        snapshot_of(&url_a, Corpus::V2)
            .iter()
            .all(|(_, v)| v.is_empty()),
        "a dry run wrote"
    );

    let full = rebuild(&one, "corpus=spam&dry_run=false").await;
    assert_eq!(full["spam"], 1, "{full}");
    let p1 = rebuild(&paged, "corpus=spam&dry_run=false&limit=4").await;
    assert_eq!(p1["next_skip"], 4, "{p1}");
    assert_eq!(p1["done"], false, "{p1}");
    let p2 = rebuild(&paged, "corpus=spam&dry_run=false&limit=4&skip=4").await;
    assert_eq!(p2["done"], true, "{p2}");
    assert_eq!(
        p1["unresolved"].as_u64().unwrap() + p2["unresolved"].as_u64().unwrap(),
        3
    );

    let whole = snapshot_of(&url_a, Corpus::V2);
    let meta = &whole[2].1;
    assert!(
        meta.contains(&(b"spam_msgs".to_vec(), b"1".to_vec()))
            && meta.contains(&(b"ham_msgs".to_vec(), b"2".to_vec())),
        "{meta:?}"
    );
    assert!(!whole[0].1.is_empty(), "no spam tokens learnt");
    assert_eq!(
        whole,
        snapshot_of(&url_b, Corpus::V2),
        "pages differ from one run"
    );

    // a page run again counts nothing twice
    rebuild(&paged, "corpus=spam&dry_run=false&limit=4").await;
    assert_eq!(
        whole,
        snapshot_of(&url_b, Corpus::V2),
        "a repeated page counted twice"
    );

    // the rebuild is of v2 alone: the legacy corpus it reads from is not
    // trained a second time
    assert!(
        snapshot_of(&url_a, Corpus::Legacy)
            .iter()
            .all(|(_, v)| v.is_empty()),
        "the rebuild wrote the legacy counts"
    );
}

/// A reader's verdict trains both corpora, each with its own tokenizer, so
/// the corpus classification reads stays current while the other is built.
#[tokio::test]
async fn a_verdict_trains_both_corpora() {
    let (_c, url) = mailrs_test_docker::kevy::kevy_server().await;
    // base64 body: the raw reading splits the encoding, the other decodes it
    let raw = b"From: s@sender.example\r\nSubject: t-x\r\n\
Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n\
aGkgdGhlcmU=\r\n"
        .to_vec();
    let url2 = url.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = kevy_client::Connection::connect(&url2).unwrap();
        mailrs_triage::spam::train(&mut c, "t-x", std::slice::from_ref(&raw), true);
        mailrs_triage::spam::train_class(&mut c, "t-x", std::slice::from_ref(&raw), "promotion");
    })
    .await
    .unwrap();
    for corpus in Corpus::ALL {
        let snap = snapshot_of(&url, corpus);
        assert!(
            snap[2].1.contains(&(b"spam_msgs".to_vec(), b"1".to_vec())),
            "{corpus:?}: {snap:?}"
        );
        assert!(!snap[0].1.is_empty(), "{corpus:?} learnt no spam tokens");
        let mut c = kevy_client::Connection::connect(&url).unwrap();
        let totals: Vec<u32> = mailrs_triage::class_totals(&mut c, corpus)
            .classes
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        assert_eq!(totals, vec![0, 0, 1], "{corpus:?}");
    }
    // each with its own tokenizer
    let legacy = snapshot_of(&url, Corpus::Legacy);
    let v2 = snapshot_of(&url, Corpus::V2);
    let has = |s: &[(String, Hash)], t: &[u8]| s[0].1.iter().any(|(k, _)| k == t);
    assert!(has(&legacy, b"agkgdghlcmu"), "{legacy:?}");
    assert!(!has(&legacy, b"there"), "{legacy:?}");
    assert!(has(&v2, b"there"), "{v2:?}");
    assert!(!has(&v2, b"agkgdghlcmu"), "{v2:?}");
}

#[tokio::test]
async fn the_triage_corpus_is_rebuilt_from_the_stored_buckets() {
    let (_c, url) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let state = seeded(&url, tmp.path());

    let dry = rebuild(&state, "corpus=triage").await;
    // t-spam is in Junk, which is not a triage class
    assert_eq!(dry["roster"], 4, "{dry}");
    assert_eq!(dry["classes"]["inbox"], 1, "{dry}");
    assert_eq!(dry["classes"]["notification"], 1, "{dry}");
    assert_eq!(dry["classes"]["promotion"], 1, "{dry}");
    assert_eq!(dry["unresolved"], 1, "t-nofile: {dry}");
    let mut c = kevy_client::Connection::connect(&url).unwrap();
    let totals = |c: &mut kevy_client::Connection| -> Vec<u32> {
        mailrs_triage::class_totals(c, Corpus::V2)
            .classes
            .into_iter()
            .map(|(_, n)| n)
            .collect()
    };
    assert_eq!(totals(&mut c), vec![0, 0, 0], "a dry run wrote");

    let a = rebuild(&state, "corpus=triage&dry_run=false&limit=2").await;
    let b = rebuild(&state, "corpus=triage&dry_run=false&skip=2").await;
    assert_eq!(b["done"], true, "{a} {b}");
    assert_eq!(totals(&mut c), vec![1, 1, 1]);
    // nothing filed: the buckets are as they were
    let promos = state
        .mailbox
        .list_thread_ids_by_bucket_unsent_via_table(
            USER,
            "promotions",
            mailrs_mailbox_kevy::ArchiveScope::All,
            10,
        )
        .unwrap();
    assert_eq!(promos, vec!["t-both2".to_string()]);
}
