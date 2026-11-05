//! fetch reaches the daemon only through the core: its reports go to the
//! core's relay, which passes them on unchanged. The daemon checks fetch's
//! own signature, so the core cannot write a fetch report; and the core's
//! count of relayed starts is what the daemon shows as fetch's counter,
//! marked as the weaker reading it is.

mod common;

use std::sync::Arc;
use std::time::Duration;

use mailrs_core_api::method::ops::PATH_OPS_RELAY;
use mailrs_core_api::method::ops_alert::OpsConfig;
use mailrs_ops::app::App;
use mailrs_ops::clock::Clock;
use mailrs_ops::config::Config;
use mailrs_ops::store::Store;
use mailrs_ops_report::push::{Config as PushConfig, Reporter};
use mailrs_ops_report::wire::REPORTS_PATH;

fn core(maildir: &std::path::Path) -> Arc<mailrs_fastcore::FastcoreState> {
    let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    let mut fc =
        mailrs_fastcore::FastcoreState::new(mailrs_mailbox_kevy::KevyMailboxStore::new(store));
    fc.net_url = Some("mem://ops-fetch-relay-side-state".into());
    fc.maildir_root = maildir.to_string_lossy().into_owned();
    Arc::new(fc)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fetch_reports_pass_through_the_core_and_only_fetch_can_sign_them() {
    let _side = kevy_client::Connection::connect("mem://ops-fetch-relay-side-state").unwrap();
    let daemon_l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let core_l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (daemon_url, core_url) = (
        format!("http://{}", daemon_l.local_addr().unwrap()),
        format!("http://{}", core_l.local_addr().unwrap()),
    );
    let maildir = tempfile::tempdir().unwrap();
    let core_router = mailrs_core_api::server::base_router_with_ops(
        core(maildir.path()),
        OpsConfig {
            relay_to: Some(format!("{daemon_url}{REPORTS_PATH}")),
            ..OpsConfig::open()
        },
    );
    tokio::spawn(async move { axum::serve(core_l, core_router).await });

    let data = tempfile::tempdir().unwrap();
    let mut cfg = Config::for_tests(data.path().to_path_buf());
    cfg.fastcore_url = core_url.clone();
    cfg.webapi_url = common::webapi(true).await;
    cfg.push_sources = vec!["fetch".into()];
    let app = App::open(
        cfg,
        Clock::System,
        Store::open(&data.path().join("ops.sqlite")).unwrap(),
    )
    .unwrap();
    let router = mailrs_ops::api::tcp_router(app.clone());
    tokio::spawn(async move { axum::serve(daemon_l, router).await });

    let before = mailrs_core_api::method::ops::fetch_starts_witnessed();
    let fetch = Reporter::start_with(
        "fetch",
        PushConfig {
            url: None,
            reports_url: Some(format!("{core_url}{PATH_OPS_RELAY}")),
            key: mailrs_ops_report::key::derive(b"test-master", "fetch"),
            state_dir: None,
        },
    );
    fetch.heartbeat(None);
    let arrived = common::until(Duration::from_secs(10), || {
        app.book
            .latest()
            .iter()
            .any(|r| r.report.process == "fetch" && r.report.seq >= 2)
    })
    .await;
    assert!(arrived, "fetch's reports did not come through the core");
    assert_eq!(
        fetch.start_count(),
        None,
        "fetch keeps no counter of its own"
    );
    assert_eq!(
        mailrs_core_api::method::ops::fetch_starts_witnessed(),
        before + 1
    );

    // the core signing as fetch with its own key: relayed, and refused
    let mut forged = fetch.build(mailrs_ops_report::wire::ReportKind::Heartbeat, None);
    forged.seq = 99;
    let body = serde_json::to_vec(&forged).unwrap();
    let core_key = mailrs_ops_report::key::derive(b"test-master", "fastcore").unwrap();
    let status = reqwest::Client::new()
        .post(format!("{core_url}{PATH_OPS_RELAY}"))
        .header(
            mailrs_ops_report::wire::SIGNATURE_HEADER,
            mailrs_ops_report::key::sign(&core_key, &body),
        )
        .body(body)
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, 401);
    assert!(app.book.latest().iter().all(|r| r.report.seq != 99));
    assert_eq!(
        app.store.rejection_counts().unwrap(),
        [("fetch".to_string(), 1)]
    );

    app.cycle().await;
    let state = app.state().await.unwrap();
    let row = state["processes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["process"] == "fetch")
        .cloned()
        .unwrap();
    assert_eq!(row["start_count"], before + 1);
    assert!(
        row["start_count_source"]
            .as_str()
            .unwrap()
            .contains("witnessed by fastcore"),
        "{row}"
    );
    assert_eq!(state["ops_status_polls"]["calls"], 1);
    assert!(state["ops_status_polls"]["last_success_at"].is_i64());
}
