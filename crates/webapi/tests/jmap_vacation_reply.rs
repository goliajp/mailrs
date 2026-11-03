//! A vacation reply set over JMAP is the one the web sets: refused for the
//! same reasons, and answering arriving mail under the same rules — once
//! per sender, never to an automatic message, a mailing list or the
//! account's own mail. Delivered through fastcore's spool drain, the
//! replies read back off the outbound queue. A real kevy-server, because
//! the queue is written with a pipeline.

mod live;

use std::sync::Arc;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_core_sidestate::families::outbound::{PENDING_IDX, job_key};
use reqwest::Method;
use serde_json::{Value, json};

const ALICE: &str = "alice@example.test";
const USING: [&str; 2] = [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:vacationresponse",
];

fn mail(id: &str, from: &str, extra: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: {ALICE}\r\nSubject: hello {id}\r\n{extra}\
         Message-ID: <{id}@sender.example>\r\nDate: Tue, 5 Aug 2025 09:00:00 +0900\r\n\r\nhi\r\n"
    )
    .into_bytes()
}

fn spool(dir: &std::path::Path, n: usize, from: &str, body: &[u8]) {
    let env = SpoolEnvelope {
        reverse_path: from.into(),
        forward_paths: vec![ALICE.into()],
        is_authenticated: false,
        auth_user: None,
        conn_id: n as u64,
        target_folder: "INBOX".into(),
        received_at: 1_790_000_000,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    };
    let new = dir.join("incoming").join("new");
    std::fs::create_dir_all(&new).expect("spool dir");
    std::fs::write(
        new.join(format!("17900000{n:02}.M{n}.test")),
        encode_spool_blob(&env, body),
    )
    .expect("spool file");
}

async fn set(live: &live::Live, token: &str, patch: Value) -> Value {
    let (status, r) = live
        .call(
            Method::POST,
            "/jmap/api/",
            token,
            Some(json!({ "using": USING, "methodCalls": [
                ["VacationResponse/set", { "update": { "singleton": patch } }, "0"]
            ] })),
        )
        .await;
    assert_eq!(status, 200, "{r}");
    r["methodResponses"][0][1].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_jmap_vacation_reply_follows_the_web_rules() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    let maildir = tmp.path().join("maildir");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "example.test");
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, "pw").await;
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(maildir.join("example.test").join("alice").join(sub)).expect("dir");
    }
    let alice = live.login(ALICE, "pw").await;

    // refused for the same reason the web refuses it
    let r = set(
        &live,
        &alice,
        json!({ "isEnabled": true, "textBody": null }),
    )
    .await;
    assert_eq!(
        r["notUpdated"]["singleton"]["description"], "invalid_body",
        "{r}"
    );
    let (status, web) = live
        .call(
            Method::PUT,
            "/api/settings/vacation",
            &alice,
            Some(json!({ "enabled": true, "subject": "", "body": "", "start": null, "end": null, "days": 7 })),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(web["error"], "invalid_body", "{web}");
    let r = set(
        &live,
        &alice,
        json!({ "isEnabled": true, "subject": "Away", "textBody": "Back on Monday." }),
    )
    .await;
    assert!(r["updated"].get("singleton").is_some(), "{r}");

    let dir = tmp.path().join("spool");
    let cases: [(&str, &str, &str); 5] = [
        ("one", "ann@sender.example", ""),
        ("two", "ann@sender.example", ""),
        (
            "auto",
            "robot@sender.example",
            "Auto-Submitted: auto-replied\r\n",
        ),
        (
            "list",
            "list@lists.example",
            "List-Id: <talk.lists.example>\r\n",
        ),
        ("own", ALICE, ""),
    ];
    for (n, (id, from, extra)) in cases.iter().enumerate() {
        spool(&dir, n, from, &mail(id, from, extra));
    }
    let mut fc = mailrs_fastcore::FastcoreState::new(live.mailbox.clone());
    fc.net_url = Some(kevy.clone());
    let fc = Arc::new(fc);
    let incoming = dir.join("incoming").join("new");
    let md = maildir.to_string_lossy().to_string();
    let drained = tokio::task::spawn_blocking(move || {
        mailrs_fastcore::spool_drain::drain_once(&incoming, &md, &fc).0
    })
    .await
    .expect("drain");
    assert_eq!(drained, 5);

    let url = kevy.clone();
    let replies = tokio::task::spawn_blocking(move || {
        let mut c = kevy_client::Connection::connect(&url).expect("kevy");
        let mut out = Vec::new();
        for id in c.lrange(PENDING_IDX, 0, -1).expect("queue") {
            let id = String::from_utf8(id).expect("id");
            let blob = c
                .hget(job_key(id).as_bytes(), b"blob")
                .expect("job")
                .expect("blob");
            let job: Value = serde_json::from_slice(&blob).expect("json");
            if job["sender"] == "<>" {
                out.push(job["recipient"].as_str().unwrap_or("").to_string());
            }
        }
        out
    })
    .await
    .expect("read queue");
    assert_eq!(
        replies,
        ["ann@sender.example"],
        "one reply, to the person, once"
    );
}
