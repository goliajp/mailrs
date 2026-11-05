//! Processes reporting to a daemon over real HTTP, with kevy-server
//! stopped: heartbeats keep arriving (the daemon never touches kevy-server),
//! a process starting over and over is reported as a crash loop from its
//! start reports alone, and starts the daemon never saw are counted from the
//! counter.

mod common;

use std::sync::Arc;
use std::time::Duration;

use mailrs_ops::app::App;
use mailrs_ops::clock::Clock;
use mailrs_ops::config::Config;
use mailrs_ops::store::Store;
use mailrs_ops_report::push::{Config as PushConfig, Reporter};

async fn daemon(data: &std::path::Path, hook: String) -> (Arc<App>, String) {
    let mut cfg = Config::for_tests(data.to_path_buf());
    cfg.webapi_url = common::webapi(false).await;
    cfg.webhook_url = Some(hook);
    cfg.heartbeat_url = Some(common::receiver().await.0);
    cfg.group_secs = 1;
    cfg.unknown_wait_secs = 3;
    cfg.push_sources = vec!["receiver".into()];
    let app = App::open(
        cfg,
        Clock::System,
        Store::open(&data.join("ops.sqlite")).unwrap(),
    )
    .unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let router = mailrs_ops::api::tcp_router(app.clone());
    tokio::spawn(async move { axum::serve(l, router).await });
    (app, format!("http://{addr}"))
}

fn push(url: Option<&str>, process: &str, dir: &std::path::Path) -> PushConfig {
    PushConfig {
        url: url.map(String::from),
        reports_url: None,
        key: mailrs_ops_report::key::derive(b"test-master", process),
        state_dir: Some(dir.to_path_buf()),
    }
}

async fn counted(app: &Arc<App>, process: &str, count: u64) {
    let ok = common::until(Duration::from_secs(10), || {
        app.book
            .latest()
            .iter()
            .any(|r| r.report.process == process && r.report.start_count == Some(count))
    })
    .await;
    assert!(ok, "{process} start {count} never arrived");
}

async fn arrived(app: &Arc<App>, process: &str, seq: u64) {
    let ok = common::until(Duration::from_secs(10), || {
        app.book
            .latest()
            .iter()
            .any(|r| r.report.process == process && r.report.seq >= seq)
    })
    .await;
    assert!(ok, "{process} report {seq} never arrived");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_kevy_server_stopped_reports_arrive_and_a_crash_loop_shows() {
    let (kevy, _url) = mailrs_test_docker::kevy::kevy_server().await;
    kevy.stop().await.expect("stop kevy-server");

    let data = tempfile::tempdir().unwrap();
    let (hook, inbox) = common::receiver().await;
    let (app, url) = daemon(data.path(), hook).await;

    let receiver_dir = tempfile::tempdir().unwrap();
    let receiver = Reporter::start_with(
        "receiver",
        push(Some(&url), "receiver", receiver_dir.path()),
    );
    receiver.heartbeat(None);
    arrived(&app, "receiver", 2).await;

    // fastcore starts three times in a row and never gets as far as ready
    let core_dir = tempfile::tempdir().unwrap();
    for n in 1..=3 {
        let r = Reporter::start_with("fastcore", push(Some(&url), "fastcore", core_dir.path()));
        counted(&app, "fastcore", n).await;
        drop(r);
    }
    let starts: u64 = app
        .store
        .all_starts_since(0)
        .unwrap()
        .iter()
        .filter(|s| s.process == "fastcore")
        .map(|s| s.starts)
        .sum();
    assert_eq!(starts, 3);

    for _ in 0..4 {
        receiver.heartbeat(None);
        app.cycle().await;
        tokio::time::sleep(Duration::from_millis(600)).await;
        app.flush().await;
    }
    let sent = common::alerts(&inbox);
    let rules: Vec<_> = sent.iter().flat_map(common::rules_in).collect();
    assert!(
        rules
            .iter()
            .any(|(r, s)| r == "crash_loop" && s == "firing"),
        "{rules:?}"
    );
    assert!(
        !rules.iter().any(|(r, _)| r == "source_unreadable.receiver"),
        "the receiver's reports kept arriving: {rules:?}"
    );
    let state = app.state().await.unwrap();
    let receiver_alert = state["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["rule"] == "source_unreadable.receiver")
        .cloned()
        .unwrap();
    assert_eq!(receiver_alert["phase"], "ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_while_the_daemon_was_away_are_counted_and_a_cleared_counter_is_a_reset() {
    let data = tempfile::tempdir().unwrap();
    let (hook, _inbox) = common::receiver().await;
    let (app, url) = daemon(data.path(), hook).await;
    let dir = tempfile::tempdir().unwrap();

    let _first = Reporter::start_with("receiver", push(Some(&url), "receiver", dir.path()));
    arrived(&app, "receiver", 1).await;
    // the daemon is away: two starts whose reports go nowhere
    for _ in 0..2 {
        let _ = Reporter::start_with("receiver", push(None, "receiver", dir.path()));
    }
    let back = Reporter::start_with("receiver", push(Some(&url), "receiver", dir.path()));
    assert_eq!(back.start_count(), Some(4));
    counted(&app, "receiver", 4).await;
    common::until(Duration::from_secs(5), || {
        app.store
            .all_starts_since(0)
            .unwrap()
            .iter()
            .any(|s| s.how == "unseen")
    })
    .await;
    let rows = app.store.all_starts_since(0).unwrap();
    let unseen: u64 = rows
        .iter()
        .filter(|s| s.how == "unseen")
        .map(|s| s.starts)
        .sum();
    assert_eq!(unseen, 2, "{rows:?}");

    std::fs::remove_file(dir.path().join("starts")).unwrap();
    let again = Reporter::start_with("receiver", push(Some(&url), "receiver", dir.path()));
    assert_eq!(again.start_count(), Some(1));
    let reset = common::until(Duration::from_secs(5), || {
        app.store
            .all_starts_since(0)
            .unwrap()
            .iter()
            .any(|s| s.how == "reset")
    })
    .await;
    assert!(reset, "{:?}", app.store.all_starts_since(0).unwrap());
}
