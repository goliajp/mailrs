//! The second round of the screening replay: every message in the window
//! rather than each conversation's newest, a hit that only observations
//! recorded after the message arrived explain kept apart from detections,
//! and what kinds of attachment each sample carried and which detectors
//! had something to read.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use tower::ServiceExt;

const KEVY: &str = "mem://screening-replay-round-two";
const BOB: &str = "bob@golia.example";

struct Letter<'a> {
    mid: &'a str,
    reply_to: Option<&'a str>,
    from_ip: &'a str,
    /// Seconds before now it arrived.
    ago: i64,
    attachment: Option<(&'a str, &'a str)>,
}

fn raw(l: &Letter<'_>) -> Vec<u8> {
    let when = chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp() - l.ago, 0)
        .unwrap()
        .to_rfc2822();
    let mid = l.mid;
    let ip = l.from_ip;
    let (threading, subject) = match l.reply_to {
        Some(parent) => (
            format!("In-Reply-To: <{parent}>\r\nReferences: <{parent}>\r\n"),
            format!("Re: about {parent}"),
        ),
        None => (String::new(), format!("about {mid}")),
    };
    let head = format!(
        "Received: from mx.sender.example ({ip}:40000) by mx.golia.example; {when}\r\n\
         Authentication-Results: mx.golia.example; spf=pass; dkim=none; dmarc=none\r\n\
         From: Sender <news@sender.example>\r\nTo: {BOB}\r\nSubject: {subject}\r\n\
         Message-ID: <{mid}>\r\n{threading}Date: {when}\r\nMIME-Version: 1.0\r\n"
    );
    let body = match l.attachment {
        None => "Content-Type: text/plain\r\n\r\nhello\r\n".to_string(),
        Some((name, content)) => format!(
            "Content-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n\
             --b\r\nContent-Type: application/octet-stream; name={name}\r\n\
             Content-Disposition: attachment; filename={name}\r\n\r\n{content}\r\n--b--\r\n"
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

async fn replay(state: &Arc<mailrs_fastcore::FastcoreState>, scope: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:screening-replay?pause_ms=0&scope={scope}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn incident(kevy: &mut kevy_client::Connection, ip: &str, at: i64) {
    mailrs_core_sidestate::families::screening::add_observation(
        kevy,
        mailrs_reputation::Observation {
            id: String::new(),
            at,
            source: mailrs_reputation::Source::Incident,
            kind: mailrs_reputation::Kind::Ip,
            value: ip.into(),
            stance: mailrs_reputation::Stance::Deny,
            retracts: None,
            reason: "seen in a phishing wave".into(),
            actor: None,
        },
    )
    .unwrap();
}

fn screening_rules(out: &serde_json::Value, mid: &str) -> Vec<(String, serde_json::Value)> {
    out["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["kind"] == "screening" && h["message_id"] == mid)
        .map(|h| {
            (
                h["rule"].as_str().unwrap().to_string(),
                h["after_the_fact"].clone(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_message_is_replayed_and_late_evidence_is_kept_apart() {
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
    let now = chrono::Utc::now().timestamp();
    let mut kevy = kevy_client::Connection::connect(KEVY).unwrap();
    // known a week before its letter arrived, and recorded a day after its
    incident(&mut kevy, "203.0.113.1", now - 7 * 86_400);
    incident(&mut kevy, "203.0.113.2", now + 86_400);

    let letters = [
        // the program sits in the middle of a conversation
        Letter {
            mid: "first@sender.example",
            reply_to: None,
            from_ip: "198.51.100.1",
            ago: 7200,
            attachment: Some(("invoice.exe", "MZ")),
        },
        Letter {
            mid: "second@sender.example",
            reply_to: Some("first@sender.example"),
            from_ip: "198.51.100.1",
            ago: 3600,
            attachment: None,
        },
        Letter {
            mid: "known@sender.example",
            reply_to: None,
            from_ip: "203.0.113.1",
            ago: 600,
            attachment: Some(("report.pdf", "%PDF-1.4 plain")),
        },
        Letter {
            mid: "late@sender.example",
            reply_to: None,
            from_ip: "203.0.113.2",
            ago: 600,
            attachment: None,
        },
    ];
    let incoming = tmp.path().join("spool/incoming/new");
    for (n, l) in letters.iter().enumerate() {
        spool(&incoming, n as u32 + 1, &raw(l));
    }
    let (drained, _) =
        mailrs_fastcore::spool_drain::drain_once(&incoming, maildir.to_str().unwrap(), &state);
    assert_eq!(drained, 4);
    assert_eq!(
        state.mailbox.all_thread_ids_for_user(BOB).unwrap().len(),
        3,
        "the reply joined its conversation"
    );

    // the first round's reading sees only each conversation's newest
    let newest = replay(&state, "newest").await;
    assert_eq!(newest["clean"]["messages"], 3, "{newest}");
    assert!(screening_rules(&newest, "first@sender.example").is_empty());
    assert!(
        newest.get("coverage").is_none(),
        "the first round's answer is unchanged"
    );
    assert!(
        newest["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h.get("after_the_fact").is_none()),
        "{newest}"
    );

    let all = replay(&state, "all").await;
    assert_eq!(all["scope"], "all");
    assert_eq!(all["clean"]["messages"], 4, "{all}");
    assert_eq!(
        screening_rules(&all, "first@sender.example"),
        [("exe-name".to_string(), serde_json::json!(false))],
        "the program in the middle of the conversation is found"
    );
    assert_eq!(
        screening_rules(&all, "known@sender.example"),
        [("ip-incident-manual".to_string(), serde_json::json!(false))]
    );
    assert_eq!(
        screening_rules(&all, "late@sender.example"),
        [("ip-incident-manual".to_string(), serde_json::json!(true))],
        "listed only after it arrived"
    );
    // detections only: the late hit is counted apart
    assert_eq!(all["clean"]["by_rule"]["ip-incident-manual"], 1, "{all}");
    assert_eq!(all["clean"]["with_hits"], 2);
    let c = &all["coverage"]["clean"];
    assert_eq!(c["after_the_fact"], 1, "{c}");
    assert_eq!(c["after_the_fact_by_rule"]["ip-incident-manual"], 1);
    assert_eq!(c["messages_with_attachments"], 2);
    assert_eq!(c["attachments"]["executable"], 1);
    assert_eq!(c["attachments"]["pdf"], 1);
    assert_eq!(c["attachments"]["zip"], 0);
    assert_eq!(c["detector_saw"]["exe-name"], 2, "any attachment is read");
    assert_eq!(c["detector_saw"]["pdf-script"], 1, "only the PDF is read");
    assert_eq!(c["detector_saw"]["office-macros"], 0);
    assert_eq!(
        c["detector_saw"]["ip-incident-manual"], 4,
        "every letter came from an address"
    );
}
