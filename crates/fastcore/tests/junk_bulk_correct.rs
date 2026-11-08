//! The one-off correction of newsletters the old score put in Junk. Every
//! message is synthetic.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const DAY: i64 = 86_400;

const NEWSLETTER: &str = "Authentication-Results: mx.x.com; spf=pass; dkim=pass; dmarc=pass\r\n\
List-Unsubscribe: <https://shop.example/u>\r\n";
const PLAIN: &str = "Authentication-Results: mx.x.com; spf=pass; dkim=pass; dmarc=pass\r\n";

struct Conv<'a> {
    tid: &'a str,
    head: &'a str,
    trust: &'a str,
    age_days: i64,
    held: bool,
    verdict: Option<String>,
}

fn conv(tid: &str) -> Conv<'_> {
    Conv {
        tid,
        head: NEWSLETTER,
        trust: "verified",
        age_days: 1,
        held: false,
        verdict: None,
    }
}

fn add(store: &KevyMailboxStore, root: &Path, c: &Conv<'_>) {
    let mid = format!("{}@shop.example", c.tid);
    let raw = format!(
        "{}From: Shop <news@shop.example>\r\nTo: bob@x.com\r\nSubject: {}\r\n\
         Message-ID: <{mid}>\r\nContent-Type: text/plain\r\n\r\nthis week's picks\r\n",
        c.head, c.tid
    );
    let md = mailrs_maildir::Maildir::open(root.join("x.com/bob"));
    let blob = md.deliver(raw.as_bytes()).unwrap().0;
    let date = chrono::Utc::now().timestamp() - c.age_days * DAY;
    let uid = store.allocate_uid(USER, &mid).unwrap();
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob,
        "sender": "news@shop.example", "recipients": USER, "subject": c.tid,
        "date": date, "internal_date": date, "size": 10,
        "flags": 0, "message_id": mid, "in_reply_to": "", "thread_id": c.tid, "modseq": 1,
        "sender_trust": c.trust,
    });
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: c.tid.into(),
                subject: c.tid.into(),
                senders_csv: "news@shop.example".into(),
                count: 1,
                unread_count: 1,
                latest_date: date,
                latest_preview: String::new(),
                category: "spam".into(),
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
            c.tid,
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
    if c.held {
        assert!(store.set_quarantined(USER, c.tid, true).unwrap());
    }
    if let Some(v) = &c.verdict {
        store.set_fraud_verdict(&mid, v).unwrap();
    }
}

fn score_verdict(score: f64, classifier: f64) -> String {
    let why = mailrs_inbound::Reason::new(mailrs_inbound::reason::JUNK_SCORE)
        .with_score("score", score)
        .with_score("threshold", 5.0)
        .with_score("content", score - classifier)
        .with_score("classifier", classifier)
        .with_score("fraud", 0.0);
    serde_json::to_string(&mailrs_inbound::FraudVerdict::placement_only(vec![why])).unwrap()
}

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
    for c in [
        conv("t-news"),
        Conv {
            verdict: Some(score_verdict(6.0, 4.0)),
            ..conv("t-scored")
        },
        Conv {
            verdict: Some(score_verdict(9.0, 4.0)),
            ..conv("t-content")
        },
        Conv {
            held: true,
            ..conv("t-held")
        },
        conv("t-mine"),
        Conv {
            age_days: 200,
            ..conv("t-old")
        },
        Conv {
            head: PLAIN,
            ..conv("t-plain")
        },
        Conv {
            trust: "unverified",
            ..conv("t-unverified")
        },
    ] {
        add(&store, root, &c);
    }
    let mut net = kevy_client::Connection::connect(url).unwrap();
    let subjects = mailrs_inbound::scan_input::JunkSubjects {
        senders: vec!["shop.example".into()],
        links: Vec::new(),
    };
    mailrs_core_sidestate::families::screening::record_user_junk(
        &mut net, USER, "t-mine", &subjects, 1,
    )
    .unwrap();
    let mut state = FastcoreState::new(store);
    state.net_url = Some(url.into());
    state.maildir_root = root.to_string_lossy().into_owned();
    Arc::new(state)
}

async fn correct(state: &Arc<FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/admin/maintenance:junk-bulk-correct?{query}"))
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

fn category(state: &FastcoreState, tid: &str) -> String {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .unwrap()
        .unwrap()
        .category
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn newsletters_the_score_junked_leave_junk_and_nothing_else_does() {
    let url = "mem://junk-bulk-correct";
    let keep = kevy_client::Connection::connect(url).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let state = seeded(url, tmp.path());

    let dry = correct(&state, "").await;
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(dry["roster"], 8, "{dry}");
    assert_eq!(dry["moved"], 2, "{dry}");
    let stayed = &dry["accounts"][USER]["stayed"];
    for (why, n) in [
        ("not_the_classifier", 1),
        ("held", 1),
        ("reader_junked", 1),
        ("out_of_window", 1),
        ("not_list_mail", 1),
        ("not_verified", 1),
    ] {
        assert_eq!(stayed[why], n, "{why}: {dry}");
    }
    assert_eq!(dry["top_sender_domains"][0][0], "shop.example", "{dry}");
    assert_eq!(category(&state, "t-news"), "spam", "a dry run moved it");

    let real = correct(&state, "dry_run=false&limit=3").await;
    let more = correct(&state, &format!("dry_run=false&skip={}", real["next_skip"])).await;
    assert_eq!(
        real["moved"].as_u64().unwrap() + more["moved"].as_u64().unwrap(),
        2,
        "{real} {more}"
    );
    assert_eq!(more["done"], true, "{more}");
    for tid in ["t-news", "t-scored"] {
        assert_eq!(category(&state, tid), "promotion", "{tid}");
    }
    for tid in [
        "t-content",
        "t-held",
        "t-mine",
        "t-old",
        "t-plain",
        "t-unverified",
    ] {
        assert_eq!(category(&state, tid), "spam", "{tid}");
    }

    let again = correct(&state, "dry_run=false").await;
    assert_eq!(again["moved"], 0, "{again}");
    assert_eq!(again["roster"], 6, "{again}");

    let mut c = kevy_client::Connection::connect(url).unwrap();
    // audited, once each
    let log = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap();
    let rows = log
        .iter()
        .filter(|r| String::from_utf8_lossy(r).contains("junk.bulk_correct"))
        .count();
    assert_eq!(rows, 2);
    // nothing trained: the move is ours, not the reader's
    let read = mailrs_triage::spam::keys(mailrs_triage::spam::READ);
    assert!(c.smembers(read.corpus_ham).unwrap().is_empty());
    assert!(c.smembers(read.corpus_spam).unwrap().is_empty());
    drop(keep);
}
