//! What activating a pack changes is measured against what is stored now.
//!
//! On 2026-11-06 a pack's gate compared the active pack with the candidate,
//! both judged by the binary then running, and predicted no change. The
//! re-judge after activation held five conversations: they had been judged
//! by an older binary that could not read the names inside a RAR archive,
//! and the running one could. Two packs judged by one binary cannot see
//! that; the stored state against the candidate's verdict can.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
// stored in the inbox, not held: judged before the running binary could
const OLD_INBOX: &str = "t-old-inbox@x.com";
// the same, filed in Junk, which is where the five of 2026-11-06 were
const OLD_JUNK: &str = "t-old-junk@x.com";
// held, and the candidate does not hold it
const FLIPPED: &str = "t-flipped@x.com";
const PLAIN_INBOX: &str = "t-plain@x.com";

// `brand-is-the-display-name`, a hold-grade rule
const FRAUD: &str = "From: Netflix <a@nowhere.invalid>\r\nSubject: hi\r\n";
const PLAIN: &str = "From: Carol <carol@lunch-partner.co.jp>\r\nSubject: lunch\r\n";

fn message(from_subject: &str) -> Vec<u8> {
    format!(
        "Received: from mx by mail.test; Sun, 01 Nov 2026 10:00:00 +0000\r\n{from_subject}\r\nbody\r\n"
    )
    .into_bytes()
}

fn add(
    state: &mailrs_fastcore::FastcoreState,
    md: &mailrs_maildir::Maildir,
    tid: &str,
    category: &str,
    raw: &[u8],
) {
    let mid = format!("m-{tid}");
    let date = 1_793_000_000;
    let blob_ref = md.deliver(raw).expect("deliver").0;
    state
        .mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category,
            is_own: false,
            latest_date: date,
            latest_preview: "body",
            senders_csv: "x",
            subject: "hi",
            thread_id: tid,
            unread: true,
            user: USER,
        })
        .expect("arrival");
    let uid = state.mailbox.allocate_uid(USER, &mid).expect("uid");
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob_ref,
        "sender": "a@nowhere.invalid", "recipients": USER, "subject": "hi",
        "date": date, "internal_date": date, "size": raw.len(),
        "flags": 0, "message_id": mid, "in_reply_to": "",
        "thread_id": tid, "modseq": 1,
    });
    state
        .mailbox
        .upsert_user_message(
            USER,
            tid,
            &mid,
            date,
            &serde_json::to_vec(&wire).expect("json"),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &blob_ref,
                uid,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("seed");
}

async fn rescan(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> (StatusCode, Vec<u8>) {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/admin/maintenance:fraud-rescan?{query}"))
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    let status = res.status();
    let body = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    (status, body.to_vec())
}

fn held(state: &mailrs_fastcore::FastcoreState, tid: &str) -> bool {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .expect("thread")
        .expect("row")
        .quarantined
}

/// Whether `pack` holds `raw`, judged as `compare` judges a file.
fn pack_holds(pack: &str, raw: &[u8]) -> bool {
    let mut rules = mailrs_fraud_lua::Rules::compile(pack).expect("pack");
    let mut scan = |f: &mailrs_fraud::Facts<'_>, p: &mailrs_fraud::Policy| {
        let e = rules.classify(f, p);
        Ok(mailrs_fraud_lua::Scan {
            findings: e.findings,
            version: String::new(),
            shadow: Vec::new(),
        })
    };
    mailrs_fraud_lua::stored::judge_with(
        raw,
        None,
        None,
        &[],
        &mailrs_fraud::Policy::default(),
        &mut None,
        &mut scan,
    )
    .expect("judge")
    .holds
}

/// One test per binary: the route reads `MAILRS_MAILDIR` and
/// `MAILRS_FRAUD_RULES_FILE` from the process environment.
#[tokio::test]
async fn a_conversation_an_older_binary_left_unheld_counts_as_a_new_hold() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("maildir");
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    let rules_dir = tmp.path().join("fraud-rules");
    std::fs::create_dir_all(&rules_dir).expect("rules dir");
    // the active pack with one comment more: a pack change that changes no rule
    let baseline = mailrs_fraud_lua::DEFAULT_SOURCE;
    let pack = format!("{baseline}\n-- reviewed\n");
    let hash = format!("{:x}", Sha256::digest(pack.as_bytes()));
    std::fs::write(rules_dir.join(format!("candidate-{hash}.lua")), &pack).expect("candidate");
    // a file whose name is not its content
    let wrong = format!("{:x}", Sha256::digest(b"another pack"));
    std::fs::write(rules_dir.join(format!("candidate-{wrong}.lua")), &pack).expect("wrong");
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", &root);
        std::env::set_var("MAILRS_FRAUD_RULES_FILE", rules_dir.join("active.lua"));
    }
    let md = mailrs_maildir::Maildir::open(&md_dir);

    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox
        .upsert_account(
            USER,
            &serde_json::json!({"address": USER, "domain": "x.com", "active": true}).to_string(),
        )
        .expect("account");
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    add(&state, &md, OLD_INBOX, "inbox", &message(FRAUD));
    add(&state, &md, OLD_JUNK, "spam", &message(FRAUD));
    add(&state, &md, FLIPPED, "inbox", &message(PLAIN));
    add(&state, &md, PLAIN_INBOX, "inbox", &message(PLAIN));
    state
        .mailbox
        .set_quarantined(USER, FLIPPED, true)
        .expect("hold");

    // Pack against pack, both judged by this binary: no hold changes.
    let changed = [message(FRAUD), message(PLAIN)]
        .iter()
        .filter(|raw| pack_holds(baseline, raw) != pack_holds(&pack, raw))
        .count();
    assert_eq!(changed, 0, "pack against pack");
    assert!(
        pack_holds(&pack, &message(FRAUD)),
        "the fixture is not hold-grade"
    );

    // Stored against candidate: the two conversations nothing held are held.
    let (status, body) = rescan(&state, &format!("candidate={hash}&pause_ms=0")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let out: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let live = &out["from_stored"];
    assert_eq!(out["dry_run"], true);
    assert_eq!(out["candidate"], format!("lua:{hash}"));
    assert_eq!(out["threads_walked"], 4);
    assert_eq!(live["would_hold"], 2, "{live}");
    assert_eq!(
        live["would_hold_from"],
        serde_json::json!({"inbox": 1, "junk": 1})
    );
    assert_eq!(live["would_release"], 1, "{live}");
    assert_eq!(live["would_release_to"], serde_json::json!({"inbox": 1}));
    assert_eq!(live["would_move_to_inbox"], 1);
    assert_eq!(live["would_move_out_of_inbox"], 1);
    assert_eq!(live["would_move_out_of_junk"], 0);
    assert_eq!(live["would_move_to_junk"], 0);
    assert_eq!(
        live["out_of_inbox_sender_domains"],
        serde_json::json!(["nowhere.invalid"])
    );
    assert_eq!(
        live["by_rule"]["hold"]["brand-is-the-display-name"], 2,
        "{live}"
    );

    // and it is dry
    assert!(!held(&state, OLD_INBOX) && !held(&state, OLD_JUNK));
    assert!(held(&state, FLIPPED));

    // Only a dry run, only a pack whose content is its name.
    let refused = [
        format!("candidate={hash}&dry_run=false"),
        format!("candidate={wrong}"),
        format!("candidate={}", "0".repeat(64)),
        "candidate=..%2Factive".to_string(),
    ];
    for q in refused {
        assert_eq!(rescan(&state, &q).await.0, StatusCode::BAD_REQUEST, "{q}");
    }
    assert!(!held(&state, OLD_INBOX), "a refused call acted");
}
