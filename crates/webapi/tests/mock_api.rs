//! The operations behind `/api/mock/*` and the `mock_*` tools, against a
//! real in-process kevy and real files. One test: the store reads its
//! configuration from the environment once per process.

use std::sync::Arc;
use std::time::Duration;

use mailrs_core_sidestate::families::mock::{self, Direction, Filter, MessageMeta};
use mailrs_webapi::WebState;
use mailrs_webapi::handlers::mock::messages::{self, WaitOutcome};
use mailrs_webapi::handlers::mock::send::{SendRequest, send};
use mailrs_webapi::handlers::mock::service::{self, MockCaller};

const DOMAIN: &str = "mock.example.test";

fn state() -> Arc<WebState> {
    Arc::new(WebState {
        // nothing listens here: an admin check fails closed, which is what
        // a caller who does not own the namespace should see
        core: Arc::new(mailrs_core_api::client::Client::new(
            "http://127.0.0.1:1",
            "test-secret",
        )),
        bind_addr: "127.0.0.1:0".into(),
        event_bus: std::sync::OnceLock::new(),
        trace_bus: std::sync::OnceLock::new(),
        receiver_stats: Arc::new(mailrs_webapi::handlers::smtp_monitor::StatsCache::default()),
        started_at: std::time::Instant::now(),
        llm_config: None,
    })
}

fn user(u: &str) -> MockCaller {
    MockCaller {
        user: u.to_string(),
        only_namespaces: None,
    }
}

// what the spool drain does for one inbound copy
fn arrive(ns: &str, tag: &str, raw: &[u8]) {
    let mut c =
        kevy_client::Connection::connect(&std::env::var("MAILRS_KEVY_URL").unwrap()).unwrap();
    let meta = MessageMeta {
        id: mock::fresh_message_id(),
        namespace: ns.into(),
        tag: tag.into(),
        direction: Direction::In,
        rcpt: format!("{ns}.{tag}@{DOMAIN}"),
        size: 0,
        received_at: 0,
        uid: 0,
        summary: mailrs_mock::parse_message(raw).summary,
    };
    mock::store_message(&mut c, mock::root(), meta, raw).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_namespace_receives_waits_reads_and_stays_its_owners() {
    let tmp = tempfile::tempdir().unwrap();
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", "mem://webapi-mock-api");
        std::env::set_var("MAILRS_MOCK_DOMAIN", DOMAIN);
        std::env::set_var("MAILRS_MOCK_ROOT", tmp.path());
    }
    let st = state();
    let ann = user("ann@example.test");

    let created = service::create_namespace(&ann, "ann-app").await.unwrap();
    assert_eq!(created.address, format!("ann-app.test@{DOMAIN}"));
    let taken = service::create_namespace(&user("bob@example.test"), "ann-app").await;
    assert_eq!(taken.unwrap_err().status, 409);

    let minted = service::new_address(&st, &ann, "ann-app", Some("signup"))
        .await
        .unwrap();
    assert_eq!(minted.address, format!("ann-app.signup@{DOMAIN}"));
    let random = service::new_address(&st, &ann, "ann-app", None)
        .await
        .unwrap();
    assert_eq!(random.tag.len(), 10);
    let bad = service::new_address(&st, &ann, "ann-app", Some("a/b")).await;
    assert_eq!(bad.unwrap_err().status, 400);

    // mail that arrived after the tag was minted but before the wait began
    tokio::time::sleep(Duration::from_millis(5)).await;
    arrive(
        "ann-app",
        "signup",
        b"From: s@svc.example\r\nSubject: code\r\n\r\nYour verification code is 731942\r\n",
    );
    let filter = Filter {
        tag: Some("signup".into()),
        ..Default::default()
    };
    let waited = messages::wait_for_message(
        &st,
        &ann,
        "ann-app",
        filter.clone(),
        Some(Duration::from_secs(2)),
    )
    .await
    .unwrap();
    let WaitOutcome::Received { message } = waited else {
        panic!("the message that came before the wait was missed");
    };
    assert_eq!(message.meta.summary.codes, ["731942"]);
    assert!(message.text.contains("731942"));

    // an explicit later `since` waits for the next one and times out
    let later = Filter {
        since: Some(i64::MAX / 2),
        ..filter.clone()
    };
    let none = messages::wait_for_message(
        &st,
        &ann,
        "ann-app",
        later,
        Some(Duration::from_millis(600)),
    )
    .await
    .unwrap();
    assert!(matches!(none, WaitOutcome::Timeout { dropped: 0 }));

    let raw = messages::read_raw(&st, &ann, "ann-app", &message.meta.id)
        .await
        .unwrap();
    assert!(raw.starts_with(b"From: s@svc.example"));

    // somebody else's namespace answers as missing, to them and to a key
    // scoped to another namespace
    let bob = user("bob@example.test");
    let hidden = messages::read_message(&st, &bob, "ann-app", &message.meta.id).await;
    assert_eq!(hidden.unwrap_err().status, 404);
    let scoped = MockCaller {
        user: "ann@example.test".into(),
        only_namespaces: Some(vec!["other".into()]),
    };
    let hidden = messages::list_messages(
        &st,
        &scoped,
        "ann-app",
        Filter {
            limit: 5,
            ..Default::default()
        },
    )
    .await;
    assert_eq!(hidden.unwrap_err().status, 404);
    assert_eq!(
        service::create_namespace(&scoped, "x-new")
            .await
            .unwrap_err()
            .status,
        403
    );
    assert!(
        service::list_namespaces(&st, &scoped, false)
            .await
            .unwrap()
            .is_empty()
    );

    // sending reaches mock recipients only
    service::create_namespace(&bob, "bob-app").await.unwrap();
    let sent = send(
        &st,
        &ann,
        "ann-app",
        SendRequest {
            from_tag: None,
            to: vec![format!("bob-app.in@{DOMAIN}"), "real@example.org".into()],
            subject: "hello".into(),
            text: "hi".into(),
            html: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(sent.delivered, [format!("bob-app.in@{DOMAIN}")]);
    assert_eq!(sent.not_delivered.len(), 1);
    assert_eq!(sent.not_delivered[0].address, "real@example.org");
    let bobs = messages::list_messages(
        &st,
        &bob,
        "bob-app",
        Filter {
            limit: 5,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(bobs.messages.len(), 1);
    assert_eq!(bobs.messages[0].summary.subject, "hello");
    let anns = messages::list_messages(
        &st,
        &ann,
        "ann-app",
        Filter {
            limit: 50,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        anns.messages
            .iter()
            .any(|m| m.direction == Direction::CapturedOut)
    );

    let removed = messages::delete_messages(&st, &ann, "ann-app", None, Some("signup"))
        .await
        .unwrap();
    assert_eq!(removed, 1);
    service::delete_namespace(&st, &ann, "ann-app")
        .await
        .unwrap();
    assert!(
        service::list_namespaces(&st, &ann, false)
            .await
            .unwrap()
            .is_empty()
    );
}
