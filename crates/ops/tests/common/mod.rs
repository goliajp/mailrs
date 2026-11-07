//! Real receivers for the daemon's smoke tests: an HTTP endpoint that
//! keeps what it is sent, a stand-in for webapi's health answer, and a
//! mailpit container as the SMTP relay.

#![allow(dead_code)] // each test binary uses its own subset

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use testcontainers::core::{ContainerPort, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

pub type Inbox = Arc<Mutex<Vec<String>>>;

async fn keep(State(i): State<Inbox>, body: Bytes) -> StatusCode {
    i.lock()
        .unwrap()
        .push(String::from_utf8_lossy(&body).into_owned());
    StatusCode::OK
}

/// `POST /` keeps the body; returns the URL and what arrived.
pub async fn receiver() -> (String, Inbox) {
    let inbox: Inbox = Arc::default();
    let app = axum::Router::new()
        .route("/", axum::routing::post(keep))
        .with_state(inbox.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await });
    (format!("http://{addr}/"), inbox)
}

/// webapi's `/api/health` and `/api/readiness` as a healthy webapi on this
/// build answers them, with `kevy` as given.
pub async fn webapi(kevy: bool) -> String {
    let now = mailrs_ops_report::build::now();
    let health = serde_json::json!({
        "status": "healthy",
        "kevy": kevy,
        "kevy_server_memory": { "used_bytes": 100, "limit_bytes": 1000 },
        "quota": { "accounts": 1, "accounts_without_limit": 0, "over_80": 0, "computed_at": now, "stale": false },
        "new_domains": { "keys": 100, "added_24h": 0, "computed_at": now, "stale": false },
        "fraud_hits": { "version": "lua:abc", "by_rule_today": {}, "by_rule_30d": {}, "computed_at": now, "stale": false },
        "jmap_uploads": { "volume_free_bytes": 500u64 << 30 },
        "set_aside": { "sender": 0 },
    });
    let app = axum::Router::new()
        .route(
            "/api/health",
            axum::routing::get(move || {
                let h = health.clone();
                async move { axum::Json(h) }
            }),
        )
        .route(
            "/api/readiness",
            axum::routing::get(|| async { StatusCode::OK }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await });
    format!("http://{addr}")
}

/// The alert messages a receiver got (digests and tests left out).
pub fn alerts(inbox: &Inbox) -> Vec<serde_json::Value> {
    inbox
        .lock()
        .unwrap()
        .iter()
        .filter_map(|b| serde_json::from_str::<serde_json::Value>(b).ok())
        .filter(|v| v["kind"] == "alerts")
        .collect()
}

pub fn rules_in(msg: &serde_json::Value) -> Vec<(String, String)> {
    msg["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["rule"].as_str().unwrap().to_string(),
                a["state"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

pub struct Mailpit {
    _container: ContainerAsync<GenericImage>,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub http: String,
}

pub async fn mailpit() -> Mailpit {
    let _startup = mailrs_test_docker::startup_lock().await;
    let container = GenericImage::new("axllent/mailpit", "latest")
        .with_exposed_port(1025.tcp())
        .with_exposed_port(8025.tcp())
        .with_wait_for(WaitFor::seconds(3))
        .with_startup_timeout(mailrs_test_docker::STARTUP_TIMEOUT)
        .start()
        .await
        .expect("start mailpit");
    let host = mailrs_test_docker::ipv4_loopback(&container.get_host().await.unwrap().to_string());
    let smtp_port = container
        .get_host_port_ipv4(ContainerPort::Tcp(1025))
        .await
        .unwrap();
    let http_port = container
        .get_host_port_ipv4(ContainerPort::Tcp(8025))
        .await
        .unwrap();
    Mailpit {
        _container: container,
        smtp_host: host.clone(),
        smtp_port,
        http: format!("http://{host}:{http_port}"),
    }
}

impl Mailpit {
    /// Subjects of the stored messages, newest first.
    pub async fn subjects(&self) -> Vec<String> {
        let v: serde_json::Value = reqwest::get(format!("{}/api/v1/messages", self.http))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        v["messages"]
            .as_array()
            .map(|m| {
                m.iter()
                    .map(|x| x["Subject"].as_str().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Wait up to `within` for `done`, checking every 200 ms.
pub async fn until(within: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = tokio::time::Instant::now() + within;
    while tokio::time::Instant::now() < end {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    done()
}
