//! Network-path integration tests for the receiver-split anti backends
//! + notification bridge, against a real kevy-server over TCP.
//!
//! The `kevy_backends` / `kevy_notify` unit tests run over `mem://`
//! (kevy's in-process dispatch), which covers every command sequence
//! but not the actual `kevy://` socket path. These spin up the real
//! `ghcr.io/goliajp/kevy` server in a container and exercise rate +
//! auth (request/response) and notify (pub/sub) over TCP.
//!
//! greylist uses the identical `with_conn` request/response mechanism
//! as rate, and its trait lives in `mailrs_shield` (not reachable from
//! this integration crate), so it is covered by the `mem://` unit test.

use std::sync::Arc;
use std::time::Duration;

use testcontainers::{
    GenericImage,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

use mailrs_server::inbound::auth_guard::{AuthCheck, AuthGuardConfig, AuthGuardStore};
use mailrs_server::inbound::kevy_backends::{KevyServerAuthGuardStore, KevyServerRateLimitStore};
use mailrs_server::inbound::rate_limit::{RateLimitStore, TokenBucketConfig};
use mailrs_server::kevy_net::KevyNetClient;
use mailrs_server::kevy_notify::{KevyEventPublisher, NOTIFY_CHANNEL, spawn_kevy_notify_bridge};
use mailrs_server::{EventBus, SmtpEvent};

/// Start a kevy-server container and return its `kevy://host:port` URL.
/// The container handle must stay alive for the duration of the test.
async fn setup_kevy() -> (testcontainers::ContainerAsync<GenericImage>, String) {
    let container = GenericImage::new("ghcr.io/goliajp/kevy", "latest")
        .with_wait_for(WaitFor::message_on_stderr("starting:"))
        .with_exposed_port(6379.tcp())
        .start()
        .await
        .expect("start kevy container");
    let host = container
        .get_host()
        .await
        .expect("container host")
        .to_string();
    // Force IPv4 loopback: docker maps the port on both 127.0.0.1 and
    // [::1], but the IPv6 proxy resets the data path here (the TCP
    // connect to ::1 still succeeds, so multi-addr fallthrough doesn't
    // help). kevy binds 0.0.0.0 (IPv4), so pin the client to it.
    let host = if host == "localhost" {
        "127.0.0.1".to_string()
    } else {
        host
    };
    let port = container
        .get_host_port_ipv4(6379)
        .await
        .expect("container port");
    let url = format!("kevy://{host}:{port}");

    // poll until the server answers PING (boot can lag the log line).
    let mut tries = 0;
    loop {
        let c = Arc::new(KevyNetClient::new(url.clone()));
        let res = tokio::task::spawn_blocking(move || c.with_conn(|conn| conn.ping()))
            .await
            .expect("join");
        match res {
            Ok(()) => break,
            Err(e) => {
                tries += 1;
                if tries >= 50 {
                    panic!("kevy server never answered PING: {e}");
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    (container, url)
}

#[tokio::test]
async fn rate_limit_over_tcp_allows_to_capacity_then_rejects() {
    let (_container, url) = setup_kevy().await;
    let client = Arc::new(KevyNetClient::new(url));
    let store = KevyServerRateLimitStore::new(
        client,
        TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.0,
        },
    );

    assert!(store.check("10.0.0.1").await, "1st under capacity");
    assert!(store.check("10.0.0.1").await, "2nd at capacity");
    assert!(!store.check("10.0.0.1").await, "3rd over capacity");
    assert!(store.check("10.0.0.2").await, "distinct key unaffected");
}

#[tokio::test]
async fn auth_guard_over_tcp_locks_then_success_clears() {
    let (_container, url) = setup_kevy().await;
    let client = Arc::new(KevyNetClient::new(url));
    let store = KevyServerAuthGuardStore::new(
        client,
        AuthGuardConfig {
            max_failures_account: 2,
            account_window_secs: 900,
            base_lockout_secs: 60,
            max_failures_ip: 1000,
            ip_window_secs: 3600,
            ip_base_lockout_secs: 3600,
            backoff_multiplier: 2.0,
            max_lockout_secs: 86400,
        },
    );
    let ip = "203.0.113.20".parse().unwrap();

    assert!(matches!(
        store.check(ip, "erin", 1_000).await,
        AuthCheck::Allowed
    ));
    store.record_failure(ip, "erin", 1_000).await;
    store.record_failure(ip, "erin", 1_000).await; // threshold → lockout
    assert!(matches!(
        store.check(ip, "erin", 1_000).await,
        AuthCheck::LockedOut { remaining_secs } if remaining_secs > 0
    ));
    store.record_success(ip, "erin").await;
    assert!(matches!(
        store.check(ip, "erin", 1_000).await,
        AuthCheck::Allowed
    ));
}

#[tokio::test]
async fn notify_bridge_delivers_over_tcp() {
    let (_container, url) = setup_kevy().await;
    let channel = NOTIFY_CHANNEL.to_vec();

    let sub_bus = EventBus::new(64);
    let mut rx = sub_bus.subscribe();
    spawn_kevy_notify_bridge(
        url.clone(),
        channel.clone(),
        "sub-origin".into(),
        sub_bus.clone(),
    );
    // let the bridge SUBSCRIBE before publishing.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let client = Arc::new(KevyNetClient::new(url));
    let publisher = KevyEventPublisher::new(client, channel, "pub-origin".into());
    publisher.publish(&SmtpEvent::NewMessage {
        user: "u".into(),
        thread_id: "t".into(),
        sender: "s".into(),
        subject: "tcp".into(),
        snippet: "sn".into(),
    });

    let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("bridge should deliver within timeout")
        .expect("recv ok");
    assert!(matches!(got.event, SmtpEvent::NewMessage { .. }));
}
