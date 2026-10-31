//! The screening replay reads stored mail the way the receiver saw it and
//! reports what the rules would have done, split into the clean sample
//! and the Junk one; it changes nothing.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use tower::ServiceExt;

const KEVY: &str = "mem://screening-replay";
const BOB: &str = "bob@golia.example";

fn letter(mid: &str, from_ip: &str, attachment: Option<&str>) -> Vec<u8> {
    let now = chrono::Utc::now().to_rfc2822();
    let head = format!(
        "Received: from mx.sender.example ({from_ip}:40000) by mx.golia.example; {now}\r\n\
         Authentication-Results: mx.golia.example; spf=pass; dkim=none; dmarc=none\r\n\
         From: Sender <news@sender.example>\r\nTo: {BOB}\r\nSubject: {mid}\r\n\
         Message-ID: <{mid}>\r\nDate: {now}\r\nMIME-Version: 1.0\r\n"
    );
    let body = match attachment {
        None => "Content-Type: text/plain\r\n\r\nhello\r\n".to_string(),
        Some(name) => format!(
            "Content-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n\
             --b\r\nContent-Type: application/octet-stream; name={name}\r\n\
             Content-Disposition: attachment; filename={name}\r\n\r\nMZ\r\n--b--\r\n"
        ),
    };
    format!("{head}{body}").into_bytes()
}

fn spool(dir: &std::path::Path, n: u32, raw: &[u8]) {
    let env = SpoolEnvelope {
        reverse_path: "news@sender.example".into(),
        forward_paths: vec![BOB.into()],
        is_authenticated: false,
        auth_user: None,
        conn_id: u64::from(n),
        target_folder: "INBOX".into(),
        received_at: 1,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    };
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join(format!("1700000000.M{n}P1.in")),
        encode_spool_blob(&env, raw),
    )
    .unwrap();
}

async fn replay(
    state: &Arc<mailrs_fastcore::FastcoreState>,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:screening-replay?pause_ms=0")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_replay_reports_both_samples_and_moves_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(maildir.join("golia.example/bob").join(sub)).unwrap();
    }
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "golia.example");
    }
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    mailbox.upsert_account(BOB, "{}").unwrap();
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    let mut kevy = kevy_client::Connection::connect(KEVY).unwrap();
    mailrs_core_sidestate::families::screening::add_observation(
        &mut kevy,
        mailrs_reputation::Observation {
            id: String::new(),
            at: chrono::Utc::now().timestamp() - 3 * 86_400,
            source: mailrs_reputation::Source::Incident,
            kind: mailrs_reputation::Kind::Ip,
            value: "203.0.113.9".into(),
            stance: mailrs_reputation::Stance::Deny,
            retracts: None,
            reason: "seen in a phishing wave".into(),
            actor: None,
        },
    )
    .unwrap();

    let incoming = tmp.path().join("spool/incoming/new");
    spool(
        &incoming,
        1,
        &letter("bad@sender.example", "203.0.113.9", Some("invoice.exe")),
    );
    spool(
        &incoming,
        2,
        &letter("fine@sender.example", "198.51.100.1", None),
    );
    let (drained, _) =
        mailrs_fastcore::spool_drain::drain_once(&incoming, maildir.to_str().unwrap(), &state);
    assert_eq!(drained, 2);
    let threads = state.mailbox.all_thread_ids_for_user(BOB).unwrap();
    assert_eq!(threads.len(), 2);
    // the plain letter goes to Junk: the control sample
    let fine = threads
        .iter()
        .find(|t| {
            let row = state.mailbox.get_thread_for_user(BOB, t).unwrap().unwrap();
            row.subject == "fine@sender.example"
        })
        .unwrap()
        .clone();
    state.mailbox.set_junk(BOB, &fine, true).unwrap();
    let before: Vec<String> = threads
        .iter()
        .map(|t| {
            state
                .mailbox
                .get_thread_for_user(BOB, t)
                .unwrap()
                .unwrap()
                .category
        })
        .collect();

    let (status, out) = replay(&state, "").await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert!(out["version"].as_str().unwrap().starts_with("lua:"));
    assert_eq!(out["approved"], false);
    assert_eq!(out["observations"], 1);
    assert_eq!(out["clean"]["messages"], 1);
    assert_eq!(out["junk"]["messages"], 1);
    assert_eq!(out["junk"]["with_hits"], 0);
    assert_eq!(out["clean"]["junk_action"], 1);
    assert_eq!(out["held"]["messages"], 0);
    assert!(out["fraud_version"].as_str().unwrap().starts_with("lua:"));
    let all = out["hits"].as_array().unwrap();
    assert!(
        all.iter()
            .all(|h| h["kind"] == "screening" || h["kind"] == "fraud")
    );
    let hits: Vec<&serde_json::Value> = all.iter().filter(|h| h["kind"] == "screening").collect();
    let rules: Vec<&str> = hits.iter().map(|h| h["rule"].as_str().unwrap()).collect();
    assert_eq!(rules, ["exe-name", "ip-incident-manual"]);
    assert!(
        hits.iter()
            .all(|h| h["message_id"] == "bad@sender.example" && h["sample"] == "clean")
    );
    assert_eq!(
        hits[1]["evidence"][0],
        "r1 (incident, 3 days): seen in a phishing wave"
    );
    let after: Vec<String> = threads
        .iter()
        .map(|t| {
            state
                .mailbox
                .get_thread_for_user(BOB, t)
                .unwrap()
                .unwrap()
                .category
        })
        .collect();
    assert_eq!(before, after, "the replay moved something");

    // a bundle under review replaces the built-in one, and a broken one is refused
    let one = format!(
        "{}\nrule({{id='only', kind='reputation', action='score', score=1, reason='r', inspired_by='', check=function(m) return 'x' end}})",
        include_str!("../../screening-lua/rules/policy.lua")
    );
    let (status, out) = replay(&state, &one).await;
    assert_eq!(status, StatusCode::OK);
    let mine = out["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["kind"] == "screening")
        .count();
    assert_eq!(mine, 2, "one hit per message");
    let (status, _) = replay(&state, "not lua !!!").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
