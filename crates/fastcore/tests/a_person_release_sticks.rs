//! A conversation a person released stays released.
//!
//! On 2026-11-03 a rule-pack change re-judged history and held four
//! conversations that had not been held, and nothing said which four: the
//! hold wrote no audit line and a release left no record. So now every hold
//! a run makes is audited, every release is kept as a fact, and a re-judge
//! holds a conversation a person released only for mail that came after.
//! A release made by a rule change is recorded too, under `system` with the
//! pack that made it, and does not stop the rules from holding it again.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const HELD: &str = "t-held@x.com";
const FLIPPED: &str = "t-flipped@x.com";

// A display name that is a company's and nothing else, from a domain that
// is not theirs: `brand-is-the-display-name`, a hold-grade rule.
const FRAUD: &str = "From: Netflix <a@nowhere.invalid>\r\nSubject: hi\r\n";
const PLAIN: &str = "From: Carol <carol@lunch-partner.co.jp>\r\nSubject: lunch\r\n";

fn message(from_subject: &str, received: &str) -> Vec<u8> {
    format!("Received: from mx by mail.test; {received}\r\n{from_subject}\r\nbody\r\n").into_bytes()
}

struct Seed<'a> {
    mailbox: &'a mailrs_mailbox_kevy::KevyMailboxStore,
    md: &'a mailrs_maildir::Maildir,
}

impl Seed<'_> {
    fn add(&self, tid: &str, mid: &str, raw: &[u8], date: i64) {
        let blob_ref = self.md.deliver(raw).expect("deliver").0;
        self.mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
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
        let uid = self.mailbox.allocate_uid(USER, mid).expect("uid");
        let wire = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob_ref,
            "sender": "a@nowhere.invalid", "recipients": USER, "subject": "hi",
            "date": date, "internal_date": date, "size": raw.len(),
            "flags": 0, "message_id": mid, "in_reply_to": "",
            "thread_id": tid, "modseq": 1,
        });
        self.mailbox
            .upsert_user_message(
                USER,
                tid,
                mid,
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
}

async fn call(
    state: &Arc<mailrs_fastcore::FastcoreState>,
    method: Method,
    uri: &str,
) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert!(res.status().is_success(), "{uri}: {}", res.status());
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

async fn rescan(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    call(
        state,
        Method::POST,
        "/v1/admin/maintenance:fraud-rescan?dry_run=false&action=hold&limit=10",
    )
    .await
}

fn held(state: &mailrs_fastcore::FastcoreState, tid: &str) -> bool {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .expect("thread")
        .expect("row")
        .quarantined
}

fn enc(s: &str) -> String {
    s.replace('@', "%40")
}

async fn audit(url: &str) -> Vec<serde_json::Value> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        let mut c = kevy_client::Connection::connect(&url).expect("kevy");
        c.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
            .expect("audit")
            .iter()
            .map(|r| serde_json::from_slice(r).expect("row"))
            .collect()
    })
    .await
    .expect("join")
}

fn lines<'a>(rows: &'a [serde_json::Value], action: &str, tid: &str) -> Vec<&'a str> {
    rows.iter()
        .filter(|r| r["action"] == action && r["target"] == tid)
        .map(|r| r["detail"].as_str().unwrap_or(""))
        .collect()
}

/// One test per binary: the route reads `MAILRS_MAILDIR` from the process
/// environment and two would race for it.
#[tokio::test]
async fn a_conversation_a_person_released_is_not_held_again_for_mail_already_there() {
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;

    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };
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
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = Some(url.clone());
    let state = Arc::new(state);
    let seed = Seed {
        mailbox: &state.mailbox,
        md: &md,
    };
    seed.add(
        HELD,
        "m-1@x.com",
        &message(FRAUD, "Sun, 01 Nov 2026 10:00:00 +0000"),
        1_793_000_000,
    );
    // held although the rules do not hold it: the rules let it go
    seed.add(
        FLIPPED,
        "m-3@x.com",
        &message(PLAIN, "Sun, 01 Nov 2026 10:00:00 +0000"),
        1_793_000_000,
    );
    state
        .mailbox
        .set_quarantined(USER, FLIPPED, true)
        .expect("hold");

    // 1. The run holds it, and says so in the audit log.
    rescan(&state).await;
    assert!(held(&state, HELD), "the run did not hold it");
    let rows = audit(&url).await;
    let holds = lines(&rows, "quarantine.hold", HELD);
    assert_eq!(holds.len(), 1, "one audit line for the new hold: {holds:?}");
    assert!(
        holds[0].contains("source=rejudge")
            && holds[0].contains("rules_version=lua:")
            && holds[0].contains("brand-is-the-display-name"),
        "the hold line names its source, pack and rule: {}",
        holds[0]
    );

    // The run let the other one go: a fact under `system`, with the pack.
    assert!(!held(&state, FLIPPED), "the run did not release it");
    let flipped = call(
        &state,
        Method::GET,
        &format!("/v1/users/{}/threads/{}/releases", enc(USER), enc(FLIPPED)),
    )
    .await;
    assert_eq!(
        flipped["items"].as_array().map(Vec::len),
        Some(1),
        "{flipped}"
    );
    assert_eq!(flipped["items"][0]["source"], "rejudge");
    assert_eq!(flipped["items"][0]["actor"], "system");
    assert!(
        flipped["items"][0]["rules_version"]
            .as_str()
            .is_some_and(|v| v.starts_with("lua:")),
        "a rule change's release names the pack: {flipped}"
    );
    let released = lines(&rows, "quarantine.release", FLIPPED);
    assert!(
        released.len() == 1
            && released[0].contains("source=rejudge")
            && released[0].contains("actor=system")
            && released[0].contains("rules_version=lua:"),
        "{released:?}"
    );

    // 2. A person releases it: a fact under their name.
    call(
        &state,
        Method::POST,
        &format!("/v1/users/{}/quarantine/{}/release", enc(USER), enc(HELD)),
    )
    .await;
    assert!(!held(&state, HELD));
    let facts = call(
        &state,
        Method::GET,
        &format!("/v1/users/{}/threads/{}/releases", enc(USER), enc(HELD)),
    )
    .await;
    assert_eq!(facts["items"].as_array().map(Vec::len), Some(1), "{facts}");
    assert_eq!(facts["items"][0]["source"], "person");
    assert_eq!(facts["items"][0]["actor"], USER);

    // 3. The same mail is judged again: it stays where the person put it.
    let tally = rescan(&state).await;
    assert!(
        !held(&state, HELD),
        "a re-judge held what a person released"
    );
    assert_eq!(tally["kept_released"], 1, "{tally}");

    // 4. Mail that came after the release may hold it again.
    seed.add(
        HELD,
        "m-2@x.com",
        &message(FRAUD, "Tue, 01 Jan 2030 10:00:00 +0000"),
        1_893_000_000,
    );
    rescan(&state).await;
    assert!(
        held(&state, HELD),
        "new mail after the release was not held"
    );

    // 5. A release by a rule change does not keep the rules out: older
    // mail that the rules now hold takes it back.
    seed.add(
        FLIPPED,
        "m-4@x.com",
        &message(FRAUD, "Sun, 01 Nov 2026 11:00:00 +0000"),
        1_793_000_100,
    );
    rescan(&state).await;
    assert!(
        held(&state, FLIPPED),
        "a rule change's release kept the rules out"
    );
}
