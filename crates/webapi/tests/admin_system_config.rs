//! System config against a real in-process kevy: what a write stores is
//! what the next read, and the audit log, see.

use axum::{
    Json,
    extract::{Extension, Path},
};
use mailrs_webapi::handlers::{conversations::AuthedUser, system_config};

fn admin() -> Extension<AuthedUser> {
    Extension(AuthedUser("lihao@golia.jp".into()))
}

fn audit_rows() -> Vec<serde_json::Value> {
    let mut c =
        kevy_client::Connection::connect(&std::env::var("MAILRS_KEVY_URL").unwrap()).unwrap();
    c.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
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
async fn system_config_round_trip() {
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_KEVY_URL", "mem://webapi-admin-system-config") };

    // system config: a REST write is audited and carries who and when
    system_config::put_system_config_key(
        admin(),
        Path("max_message_size_bytes".into()),
        Json(system_config::SetSystemConfigRequest {
            value: "1048576".into(),
        }),
    )
    .await
    .unwrap();
    let Json(cfg) = system_config::get_system_config().await.unwrap();
    let e = entry(&cfg, "max_message_size_bytes");
    assert_eq!(e["value"], "1048576");
    assert_eq!(e["source"], "database");
    assert_eq!(e["updated_by"], "lihao@golia.jp");
    assert!(e["updated_at"].as_i64().unwrap() > 1_700_000_000);
    // a key never overridden says nothing about who changed it
    assert!(entry(&cfg, "smtp_port").get("updated_at").is_none());

    system_config::delete_system_config_key(admin(), Path("max_message_size_bytes".into()))
        .await
        .unwrap();
    let Json(cfg) = system_config::get_system_config().await.unwrap();
    let e = entry(&cfg, "max_message_size_bytes");
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
    let key = "max_message_size_bytes".to_string();
    assert!(actions.contains(&("system_config.set".into(), key.clone())));
    assert!(actions.contains(&("system_config.reset".into(), key)));
}
