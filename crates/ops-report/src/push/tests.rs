use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};

use super::*;

#[derive(Default)]
struct Received {
    reports: Vec<(Report, String)>,
    refuse_first: usize,
    seen: usize,
}

type Shared = Arc<Mutex<Received>>;

async fn receive(State(r): State<Shared>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let mut r = r.lock().unwrap();
    r.seen += 1;
    if r.seen <= r.refuse_first {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    let sig = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let report: Report = serde_json::from_slice(&body).unwrap();
    assert!(crate::key::verify(b"master", &report.process, &body, &sig));
    r.reports.push((report, sig));
    StatusCode::NO_CONTENT
}

async fn daemon(refuse_first: usize) -> (String, Shared) {
    let shared: Shared = Arc::new(Mutex::new(Received {
        refuse_first,
        ..Default::default()
    }));
    let app = axum::Router::new()
        .route(REPORTS_PATH, axum::routing::post(receive))
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    (format!("http://{addr}"), shared)
}

async fn settle(shared: &Shared, want: usize, within: Duration) {
    let until = tokio::time::Instant::now() + within;
    while shared.lock().unwrap().reports.len() < want && tokio::time::Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn cfg(url: &str, dir: &std::path::Path) -> Config {
    Config {
        url: Some(url.to_string()),
        reports_url: None,
        key: crate::key::derive(b"master", "receiver"),
        state_dir: Some(dir.to_path_buf()),
    }
}

#[tokio::test]
async fn start_ready_and_heartbeat_arrive_signed_and_in_order() {
    let (url, shared) = daemon(0).await;
    let dir = tempfile::tempdir().unwrap();
    let r = Reporter::start_with("receiver", cfg(&url, dir.path()));
    settle(&shared, 1, Duration::from_secs(5)).await;
    r.ready(None);
    settle(&shared, 2, Duration::from_secs(5)).await;
    r.heartbeat(None);
    settle(&shared, 3, Duration::from_secs(5)).await;
    let got = shared.lock().unwrap();
    let kinds: Vec<_> = got.reports.iter().map(|(r, _)| r.kind).collect();
    assert_eq!(
        kinds,
        [ReportKind::Start, ReportKind::Ready, ReportKind::Heartbeat]
    );
    let seqs: Vec<_> = got.reports.iter().map(|(r, _)| r.seq).collect();
    assert_eq!(seqs, [1, 2, 3]);
    assert!(got.reports.iter().all(|(r, _)| r.start_count == Some(1)));
    assert!(counter::alive_age(dir.path(), crate::build::now()).is_some());
}

#[tokio::test]
async fn a_report_the_daemon_could_not_take_is_tried_again() {
    let (url, shared) = daemon(1).await;
    let dir = tempfile::tempdir().unwrap();
    let _r = Reporter::start_with("receiver", cfg(&url, dir.path()));
    settle(&shared, 1, Duration::from_secs(10)).await;
    let got = shared.lock().unwrap();
    assert_eq!(got.seen, 2, "one refusal, then the retry");
    assert_eq!(got.reports[0].0.kind, ReportKind::Start);
}

#[tokio::test]
async fn without_a_key_nothing_is_sent_but_the_start_is_counted() {
    let (url, shared) = daemon(0).await;
    let dir = tempfile::tempdir().unwrap();
    let mut c = cfg(&url, dir.path());
    c.key = None;
    let r = Reporter::start_with("receiver", c);
    r.heartbeat(None);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(shared.lock().unwrap().seen, 0);
    assert_eq!(r.start_count(), Some(1));
    assert!(counter::alive_age(dir.path(), crate::build::now()).is_some());
}
