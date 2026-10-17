//! Apps and system config against a real in-process kevy: what a write
//! stores is what the next read, and the audit log, see.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use mailrs_webapi::WebState;
use mailrs_webapi::handlers::{apps_keys, conversations::AuthedUser, system_config};

fn state() -> Arc<WebState> {
    Arc::new(WebState {
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

fn admin() -> Extension<AuthedUser> {
    Extension(AuthedUser("lihao@golia.jp".into()))
}

fn audit_rows() -> Vec<serde_json::Value> {
    let mut c =
        kevy_client::Connection::connect(&std::env::var("MAILRS_KEVY_URL").unwrap()).unwrap();
    c.lrange(b"admin:audit_log", 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(r).unwrap())
        .collect()
}

fn entry(cfg: &serde_json::Value, key: &str) -> serde_json::Value {
    cfg["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["key"] == key)
        .cloned()
        .unwrap_or_else(|| panic!("no entry {key}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn app_scopes_and_system_config_round_trip() {
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_KEVY_URL", "mem://webapi-admin-apps-config") };

    // apps: the created secret comes back once, the list never carries it
    let Json(created) = apps_keys::create_app(Json(apps_keys::CreateAppRequest {
        name: "ci-bot".into(),
        scopes: vec!["mail.read".into(), "mail.send".into()],
    }))
    .await
    .unwrap();
    let app_id = created["app_id"].as_str().unwrap().to_string();
    assert_eq!(created["secret"].as_str().unwrap().len(), 64);

    let Json(list) = apps_keys::list_apps().await.unwrap();
    let row = &list["items"][0];
    assert_eq!(row["app_id"], app_id.as_str());
    assert_eq!(row["scopes"], serde_json::json!(["mail.read", "mail.send"]));
    assert!(row.get("secret_sha256").is_none());
    assert!(row.get("secret").is_none());

    // an edited scope list is what the app record, the one place scopes live, holds
    let status = apps_keys::set_app_scopes(
        State(state()),
        admin(),
        Path(app_id.clone()),
        Json(apps_keys::AppScopesRequest {
            scopes: vec!["mail.read".into()],
        }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    let Json(app) = apps_keys::get_app(Path(app_id.clone())).await.unwrap();
    assert_eq!(app["scopes"], serde_json::json!(["mail.read"]));
    assert_eq!(app["name"], "ci-bot");

    let missing = apps_keys::set_app_scopes(
        State(state()),
        admin(),
        Path("app_999".into()),
        Json(apps_keys::AppScopesRequest { scopes: vec![] }),
    )
    .await;
    assert_eq!(missing.unwrap_err(), StatusCode::NOT_FOUND);

    // system config: a REST write is audited and carries who and when
    system_config::put_system_config_key(
        admin(),
        Path("hostname".into()),
        Json(system_config::SetSystemConfigRequest {
            value: "mx.example.test".into(),
        }),
    )
    .await
    .unwrap();
    let Json(cfg) = system_config::get_system_config().await.unwrap();
    let e = entry(&cfg, "hostname");
    assert_eq!(e["value"], "mx.example.test");
    assert_eq!(e["source"], "database");
    assert_eq!(e["updated_by"], "lihao@golia.jp");
    assert!(e["updated_at"].as_i64().unwrap() > 1_700_000_000);
    // a key never overridden says nothing about who changed it
    assert!(entry(&cfg, "smtp_port").get("updated_at").is_none());

    system_config::delete_system_config_key(admin(), Path("hostname".into()))
        .await
        .unwrap();
    let Json(cfg) = system_config::get_system_config().await.unwrap();
    let e = entry(&cfg, "hostname");
    assert_ne!(e["source"], "database");
    assert!(e.get("updated_by").is_none());

    let actions: Vec<(String, String)> = audit_rows()
        .iter()
        .map(|r| {
            (
                r["action"].as_str().unwrap().to_string(),
                r["target"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(actions.contains(&("app.scopes_update".into(), app_id)));
    assert!(actions.contains(&("system_config.set".into(), "hostname".into())));
    assert!(actions.contains(&("system_config.reset".into(), "hostname".into())));
}
