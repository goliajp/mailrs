//! The System Config page stores only what a running process reads.
//! Ports, hostname and TLS are read once at start from compose, so a
//! write to them is refused with where they are set, and nothing is
//! stored.

use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
};
use mailrs_webapi::handlers::{conversations::AuthedUser, system_config};

const KEVY: &str = "mem://webapi-system-config-truth";

fn admin() -> Extension<AuthedUser> {
    Extension(AuthedUser("lihao@golia.jp".into()))
}

async fn put(key: &str, value: &str) -> Result<StatusCode, system_config::Refused> {
    system_config::put_system_config_key(
        admin(),
        Path(key.into()),
        Json(system_config::SetSystemConfigRequest {
            value: value.into(),
        }),
    )
    .await
}

fn stored() -> Vec<Vec<u8>> {
    let mut c = kevy_client::Connection::connect(KEVY).unwrap();
    c.hgetall(mailrs_core_sidestate::keys::admin::SYSTEM_CONFIG)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_only_settings_answer_400_and_store_nothing() {
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };

    for key in [
        "hostname",
        "smtp_port",
        "submission_port",
        "imap_port",
        "tls_enabled",
    ] {
        let (status, Json(body)) = put(key, "1").await.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST, "{key}");
        let why = body["error"].as_str().unwrap();
        assert!(
            why.contains("compose") && why.contains("restarting"),
            "{key}: {why}"
        );
    }
    let (status, Json(body)) = put("local_domains", "a.example").await.unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("Domains page"));
    let (status, _) = put("max_message_size_bytes", "0").await.unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(stored().is_empty(), "a refused write was stored");

    assert_eq!(
        put("max_message_size_bytes", "2048").await.unwrap(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(stored().len(), 2);

    let Json(cfg) = system_config::get_system_config().await.unwrap();
    let entries = cfg["entries"].as_array().unwrap();
    let keys: Vec<&str> = entries.iter().map(|e| e["key"].as_str().unwrap()).collect();
    assert!(!keys.contains(&"local_domains"), "{keys:?}");
    for e in entries {
        let editable = e["key"] == "max_message_size_bytes";
        assert_eq!(e["editable"], editable, "{e}");
    }
    let tls = entries.iter().find(|e| e["key"] == "tls_enabled").unwrap();
    assert!(
        tls["value"].is_null(),
        "webapi cannot see the certificate paths: {tls}"
    );
    let size = entries
        .iter()
        .find(|e| e["key"] == "max_message_size_bytes")
        .unwrap();
    assert_eq!(size["value"], "2048");
    assert_eq!(
        size["default_value"],
        mailrs_smtp_proto::MAX_MESSAGE_SIZE.to_string()
    );
}
