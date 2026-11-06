//! The own path on a real core: the daemon's derived key queues an alert
//! to the configured operator addresses and nowhere else, a request that
//! names recipients is refused, and the key opens no mailbox route.

use std::sync::Arc;

use mailrs_core_api::method::ops::OpsAuth;
use mailrs_core_api::method::ops_alert::{OpsConfig, OpsOperators, PATH_OPS_ALERTS};
use mailrs_core_sidestate::families::outbound::{OPS_ALERT_ID, PENDING_IDX, job_key};
use sha2::Digest;

fn core(maildir: &std::path::Path, side: &str) -> Arc<mailrs_fastcore::FastcoreState> {
    let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    let mut fc =
        mailrs_fastcore::FastcoreState::new(mailrs_mailbox_kevy::KevyMailboxStore::new(store));
    fc.net_url = Some(side.into());
    fc.maildir_root = maildir.to_string_lossy().into_owned();
    Arc::new(fc)
}

async fn serve(router: axum::Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, router).await });
    format!("http://{addr}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ops_key_queues_to_the_operators_only_and_reads_nothing() {
    // a kevy-server: the jobs are written with a pipeline
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
    let mut side = kevy_client::Connection::connect(&url).unwrap();
    let maildir = tempfile::tempdir().unwrap();
    let state = core(maildir.path(), &url);
    let key = mailrs_ops_report::key::derive(b"master", "ops").unwrap();
    let cfg = OpsConfig {
        alerts: OpsAuth::TokenSha256(hex::encode(sha2::Sha256::digest(key.as_bytes()))),
        operators: Some(OpsOperators {
            from: "ops-alerts@example.test".into(),
            to: vec!["oncall@example.test".into()],
        }),
        ..OpsConfig::open()
    };
    let ops_url = serve(mailrs_core_api::server::base_router_with_ops(
        state.clone(),
        cfg,
    ))
    .await;
    let http = reqwest::Client::new();
    let post = |body: serde_json::Value, bearer: Option<&str>| {
        let mut r = http.post(format!("{ops_url}{PATH_OPS_ALERTS}")).json(&body);
        if let Some(b) = bearer {
            r = r.bearer_auth(b);
        }
        r.send()
    };
    let alert = serde_json::json!({"id": "1800000000-1", "subject": "s", "text": "t"});
    assert_eq!(post(alert.clone(), None).await.unwrap().status(), 401);
    assert_eq!(
        post(alert.clone(), Some("not-the-key"))
            .await
            .unwrap()
            .status(),
        401
    );
    let naming = serde_json::json!({"id": "x", "subject": "s", "text": "t", "to": ["attacker@elsewhere.test"]});
    assert_eq!(
        post(naming, Some(&key)).await.unwrap().status(),
        400,
        "no recipients from the caller"
    );
    let r = post(alert, Some(&key)).await.unwrap();
    let status = r.status();
    let answer = r.text().await.unwrap();
    assert_eq!(status, 200, "{answer}");
    assert!(answer.contains(r#""queued":1"#), "{answer}");

    let ids = side.lrange(PENDING_IDX, 0, -1).unwrap();
    assert_eq!(ids.len(), 1, "one job, for the one operator address");
    let blob = side
        .hget(
            job_key(String::from_utf8(ids[0].clone()).unwrap()).as_bytes(),
            b"blob",
        )
        .unwrap()
        .unwrap();
    let job: serde_json::Value = serde_json::from_slice(&blob).unwrap();
    assert_eq!(job["recipient"], "oncall@example.test");
    assert_eq!(job[OPS_ALERT_ID], "1800000000-1");

    // every call to the route, refused or queued, is in the audit log
    let audited: Vec<String> = side
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| {
            serde_json::from_slice::<mailrs_core_sidestate::families::audit::AuditFact>(r).ok()
        })
        .map(|f| format!("{} {}", f.actor, f.action))
        .collect();
    assert_eq!(
        audited
            .iter()
            .filter(|a| *a == "ops-daemon ops.alert.refused")
            .count(),
        3,
        "{audited:?}"
    );
    assert_eq!(
        audited
            .iter()
            .filter(|a| *a == "ops-daemon ops.alert.queued")
            .count(),
        1,
        "{audited:?}"
    );

    // the same key against the core's mailbox routes
    let full = serve(mailrs_fastcore::build_router_with_secret(
        state,
        "core-secret".into(),
    ))
    .await;
    for path in [
        "/v1/admin/accounts",
        "/v1/users/oncall@example.test/conversations/unseen-count",
    ] {
        let status = http
            .get(format!("{full}{path}"))
            .bearer_auth(&key)
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, 401, "{path}");
    }
}
