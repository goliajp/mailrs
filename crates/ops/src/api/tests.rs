//! The daemon's routes over real sockets: reports from the real sender,
//! the administrator routes behind the shared secret, and the CLI's socket.

use std::sync::Arc;

use sha2::Digest;

use crate::app::App;
use crate::clock::Clock;
use crate::config::Config;
use crate::store::Store;

const NOW: i64 = 1_800_000_000;

async fn serve(app: Arc<App>) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let router = super::tcp_router(app);
    tokio::spawn(async move { axum::serve(l, router).await });
    format!("http://{addr}")
}

fn app(dir: &std::path::Path, clock: Clock) -> Arc<App> {
    let mut cfg = Config::for_tests(dir.to_path_buf());
    cfg.admin_secret_sha256 = Some(hex::encode(sha2::Sha256::digest(b"admin-secret")));
    App::open(cfg, clock, Store::in_memory().unwrap()).unwrap()
}

#[tokio::test]
async fn a_process_report_from_the_real_sender_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), Clock::System);
    let url = serve(app.clone()).await;
    let reporter = mailrs_ops_report::push::Reporter::start_with(
        "receiver",
        mailrs_ops_report::push::Config {
            url: Some(url),
            key: mailrs_ops_report::key::derive(b"test-master", "receiver"),
            state_dir: Some(dir.path().join("receiver")),
        },
    );
    reporter.ready(None);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.book.latest().first().map(|r| r.report.seq) != Some(2)
        && std::time::Instant::now() < until
    {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let latest = app.book.latest();
    assert_eq!(latest.len(), 1);
    assert_eq!(
        latest[0].report.kind,
        mailrs_ops_report::wire::ReportKind::Ready
    );
    assert_eq!(latest[0].report.start_count, Some(1));
}

#[tokio::test]
async fn administrator_routes_need_the_shared_secret_and_a_named_admin() {
    let dir = tempfile::tempdir().unwrap();
    let url = serve(app(dir.path(), Clock::manual(NOW))).await;
    let http = reqwest::Client::new();
    let state = |secret: Option<&str>| {
        let mut r = http.get(format!("{url}/v1/state"));
        if let Some(s) = secret {
            r = r.header(super::ADMIN_SECRET_HEADER, s);
        }
        r.send()
    };
    assert_eq!(state(None).await.unwrap().status(), 401);
    assert_eq!(state(Some("wrong")).await.unwrap().status(), 401);
    assert_eq!(state(Some("admin-secret")).await.unwrap().status(), 200);
    let silence = |actor: Option<&str>, body: serde_json::Value| {
        let mut r = http
            .post(format!("{url}/v1/silences"))
            .header(super::ADMIN_SECRET_HEADER, "admin-secret")
            .json(&body);
        if let Some(a) = actor {
            r = r.header(super::ACTOR_HEADER, a);
        }
        r.send()
    };
    let ok = serde_json::json!({"rule": "spool_quarantined", "for_secs": 3600, "reason": "known"});
    assert_eq!(
        silence(None, ok.clone()).await.unwrap().status(),
        400,
        "nobody named"
    );
    let no_end = serde_json::json!({"rule": "spool_quarantined", "reason": "known"});
    assert_eq!(
        silence(Some("alice"), no_end).await.unwrap().status(),
        400,
        "a silence must end"
    );
    let r = silence(Some("alice"), ok).await.unwrap();
    assert_eq!(r.status(), 200);
    let s: serde_json::Value = r.json().await.unwrap();
    assert_eq!(s["by"], "webapi:alice");
    assert_eq!(s["until"], NOW + 3600);
}

#[tokio::test]
async fn the_socket_serves_the_cli_and_maintenance_ends_on_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::manual(NOW);
    let app = app(dir.path(), clock.clone());
    let sock = dir.path().join("ops.sock");
    let l = tokio::net::UnixListener::bind(&sock).unwrap();
    let router = super::socket_router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await });
    let call = |method: &'static str, uri: &'static str, body: Option<serde_json::Value>| {
        let sock = sock.clone();
        async move {
            crate::cli::call(&sock, method, uri, body.as_ref())
                .await
                .unwrap()
        }
    };
    let start = serde_json::json!({"action": "start", "for_secs": 600, "reason": "upgrade"});
    assert_eq!(
        call("POST", "/v1/maintenance", Some(start.clone())).await.0,
        400,
        "--by is required"
    );
    let start =
        serde_json::json!({"action": "start", "for_secs": 600, "reason": "upgrade", "by": "bob"});
    let (status, body) = call("POST", "/v1/maintenance", Some(start)).await;
    assert_eq!(status, 200, "{body}");
    assert!(app.store.maintenance(NOW).unwrap().is_some());
    clock.advance(601);
    app.cycle().await;
    assert!(app.store.maintenance(clock.now()).unwrap().is_none());
    let (_, history) = call("GET", "/v1/history?since=0", None).await;
    let v: serde_json::Value = serde_json::from_str(&history).unwrap();
    let actions: Vec<_> = v["controls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            format!(
                "{} by {}",
                c["action"].as_str().unwrap(),
                c["by"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(actions, ["expired by ops-daemon", "start by cli:bob"]);
    let (status, _) = call("GET", "/v1/state", None).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn channels_are_tested_on_the_first_cycle_then_weekly_and_after_a_change() {
    use std::sync::{Arc as A, Mutex};
    let got: A<Mutex<Vec<String>>> = A::default();
    let keep = got.clone();
    let hook = axum::Router::new().route(
        "/",
        axum::routing::post(move |body: String| {
            let keep = keep.clone();
            async move {
                keep.lock().unwrap().push(body);
                "ok"
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, hook).await });
    let tests = || {
        got.lock()
            .unwrap()
            .iter()
            .filter(|b| b.contains("\"kind\":\"test\""))
            .count()
    };
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("ops.sqlite");
    let clock = Clock::manual(NOW);
    let mut cfg = Config::for_tests(dir.path().to_path_buf());
    cfg.webhook_url = Some(url.clone());
    let app = App::open(
        cfg.clone(),
        clock.clone(),
        Store::open(&store_path).unwrap(),
    )
    .unwrap();
    app.cycle().await;
    assert_eq!(tests(), 1);
    clock.advance(86_400);
    app.cycle().await;
    assert_eq!(tests(), 1, "not again within the week");
    clock.advance(6 * 86_400);
    app.cycle().await;
    assert_eq!(tests(), 2, "weekly");
    drop(app);
    // restarted with another channel configured: tested at once
    cfg.webhook_url = Some(format!("{url}?changed"));
    let app = App::open(cfg, clock.clone(), Store::open(&store_path).unwrap()).unwrap();
    app.cycle().await;
    assert_eq!(tests(), 3);
}
