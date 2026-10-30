//! A send cannot label itself a security alert: a `kind` in the request
//! is not written onto the job, so the sender counts and holds it like
//! any mail. A paused account's send is refused with 423, saying whom to
//! ask; a super administrator lists the paused accounts and releases one,
//! sending or discarding what was held, and the answer counts each.
//!
//! Against a real kevy-server: the queue write is pipelined.

mod live;

use mailrs_core_sidestate::families::outbound::{self as ob, pause};
use reqwest::Method;
use serde_json::json;

const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paused_account_is_told_and_an_administrator_releases_it() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
        std::env::set_var("MAILRS_RESET_CONTACT_NAME", "Li Hao");
        std::env::set_var("MAILRS_RESET_CONTACT", "lihao@example.test");
    }
    let live = live::start(&kevy).await;
    for who in [ADMIN, ALICE] {
        live.add_account(who, "pw").await;
    }
    live.make_super(ADMIN);
    let admin = live.login(ADMIN, "pw").await;
    live.confirm(&admin, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let mut c = kevy_client::Connection::connect(&kevy).unwrap();

    // a kind in the request does not reach the job
    let mail = json!({
        "from": ALICE, "to": ["bob@example.test"], "subject": "hi", "body": "x",
        "kind": ob::SECURITY_ALERT_KIND,
    });
    let (status, got) = live
        .call(Method::POST, "/api/mail/send", &alice, Some(mail.clone()))
        .await;
    assert_eq!(status, 200, "{got}");
    let mut kinds = Vec::new();
    for key in c.keys(ob::JOB_PATTERN).unwrap() {
        let blob = c.hget(&key, b"blob").unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&blob).unwrap();
        kinds.push(v.get("kind").cloned());
    }
    assert_eq!(kinds, vec![None], "a sender chose its job's kind");

    // the sender paused alice and held a job of hers
    pause::pause(&mut c, ALICE, "201 recipients in an hour", 1_000).unwrap();
    let held = ob::write_fresh_pending(
        &mut c,
        &ob::FreshPending {
            sender: ALICE,
            recipient: "bob@example.test",
            message_data_base64: "aGk=",
            scheduled_at: None,
            original_sender: None,
            send_id: None,
            account: None,
        },
        1_000,
    )
    .unwrap();
    pause::hold(&mut c, ALICE, &held.to_string(), 1_000).unwrap();

    let (status, got) = live
        .call(Method::POST, "/api/mail/send", &alice, Some(mail))
        .await;
    assert_eq!(
        (status, got["error"].as_str()),
        (423, Some("outbound_paused"))
    );
    let message = got["message"].as_str().unwrap_or_default();
    assert!(message.contains("Li Hao <lihao@example.test>"), "{message}");

    let (status, list) = live
        .call(Method::GET, "/api/admin/outbound/paused", &admin, None)
        .await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list["items"][0]["address"], ALICE);
    assert_eq!(list["items"][0]["held"], 1);
    // only a super administrator sees it
    let (status, _) = live
        .call(Method::GET, "/api/admin/outbound/paused", &alice, None)
        .await;
    assert_eq!(status, 403);

    let resume = format!("/api/admin/outbound/paused/{ALICE}/resume");
    let (status, got) = live
        .call(
            Method::POST,
            &resume,
            &admin,
            Some(json!({ "held": "send" })),
        )
        .await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(
        (got["sent"].as_u64(), got["discarded"].as_u64()),
        (Some(1), Some(0))
    );
    let state = c.hget(ob::job_key(held).as_bytes(), b"state").unwrap();
    assert_eq!(state.as_deref(), Some(b"pending".as_slice()));
    assert!(!pause::is_paused(&mut c, ALICE).unwrap());
    let (status, _) = live
        .call(
            Method::POST,
            &resume,
            &admin,
            Some(json!({ "held": "send" })),
        )
        .await;
    assert_eq!(status, 404, "resuming an account not paused");

    let audit: Vec<serde_json::Value> = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice(r).ok())
        .collect();
    assert!(audit.iter().any(|r| r["action"] == "outbound.resume"
        && r["target"] == ALICE
        && r["detail"] == "sent=1 discarded=0"));
}
