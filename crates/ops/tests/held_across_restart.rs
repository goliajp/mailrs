//! A notice held back during maintenance is kept in the daemon's store,
//! so the summary sent when maintenance ends still lists it after the
//! daemon restarted in between.

mod common;

use std::sync::Arc;
use std::time::Duration;

use mailrs_ops::app::App;
use mailrs_ops::clock::Clock;
use mailrs_ops::config::Config;
use mailrs_ops::engine::Notice;
use mailrs_ops::engine::machine::NoticeKind;
use mailrs_ops::store::Store;
use sha2::Digest;

const SECRET: &str = "admin-secret-for-the-test";

async fn start(
    data: &std::path::Path,
    hook: &str,
) -> (Arc<App>, String, tokio::task::JoinHandle<()>) {
    let mut cfg = Config::for_tests(data.to_path_buf());
    cfg.webhook_url = Some(hook.to_string());
    cfg.group_secs = 1;
    cfg.admin_secret_sha256 = Some(hex::encode(sha2::Sha256::digest(SECRET.as_bytes())));
    let app = App::open(
        cfg,
        Clock::System,
        Store::open(&data.join("ops.sqlite")).unwrap(),
    )
    .unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let router = mailrs_ops::api::tcp_router(app.clone());
    let server = tokio::spawn(async move {
        let _ = axum::serve(l, router).await;
    });
    (app, format!("http://{addr}"), server)
}

async fn admin(
    base: &str,
    method: reqwest::Method,
    path: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let r = reqwest::Client::new()
        .request(method, format!("{base}{path}"))
        .header("x-mailrs-ops-admin", SECRET)
        .header("x-mailrs-ops-actor", "admin@example.test")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{path}: {}", r.status());
    r.json().await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn held_notices_reach_the_summary_after_a_restart() {
    let data = tempfile::tempdir().unwrap();
    let (hook, inbox) = common::receiver().await;

    let (app, base, server) = start(data.path(), &hook).await;
    admin(
        &base,
        reqwest::Method::POST,
        "/v1/maintenance",
        serde_json::json!({ "action": "start", "for_secs": 3600, "reason": "upgrade" }),
    )
    .await;
    let rule = mailrs_ops::rules::all()
        .into_iter()
        .find(|r| r.name == "spool_quarantined")
        .unwrap();
    assert!(
        !rule.during_maintenance,
        "the rule must be one maintenance holds"
    );
    let now = app.clock.now();
    let notice = Notice {
        rule,
        object: "spool".into(),
        kind: NoticeKind::Firing,
        severity: rule.severity,
        value: "2".into(),
        threshold: "0".into(),
        since: now,
        at: now,
        under: None,
    };
    {
        let mut d = app.dispatcher.lock().await;
        d.accept(&[notice], &[], true, now);
        d.tick(now + 5).await;
    }
    assert!(inbox.lock().unwrap().is_empty(), "held, not sent");

    // the daemon goes away and comes back on the same store
    server.abort();
    let _ = server.await;
    drop(app);
    let (_app, base, _server) = start(data.path(), &hook).await;

    let state = admin(
        &base,
        reqwest::Method::GET,
        "/v1/state",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(state["notifications"]["held_for_maintenance"], 1, "{state}");

    admin(
        &base,
        reqwest::Method::POST,
        "/v1/maintenance",
        serde_json::json!({ "action": "end", "reason": "done" }),
    )
    .await;
    let arrived = common::until(Duration::from_secs(10), || {
        !inbox.lock().unwrap().is_empty()
    })
    .await;
    assert!(arrived, "no summary at the end of maintenance");
    let got = inbox.lock().unwrap();
    let summary: serde_json::Value = serde_json::from_str(&got[0]).unwrap();
    assert_eq!(summary["kind"], "maintenance_ended", "{summary}");
    let alerts = summary["alerts"].as_array().unwrap();
    assert_eq!(alerts.len(), 1, "{summary}");
    assert_eq!(alerts[0]["rule"], "spool_quarantined");
    assert_eq!(alerts[0]["object"], "spool");
    assert_eq!(alerts[0]["value"], "2");
    assert_eq!(alerts[0]["state"], "firing");
}
